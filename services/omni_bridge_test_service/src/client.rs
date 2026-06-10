//! Client mode – run the bridge as a stdin/stdout-driven RPC server.
//!
//! In this mode we register every service exported by the
//! [`bridge_rpc_services`] crate against a [`bridge_rpc_router::Router`] and
//! drive a [`BridgeRpc`](bridge_rpc_core::BridgeRpc) over the process's own
//! stdin/stdout.
//!
//! # Stdout discipline
//!
//! Stdout is reserved for RPC frames – it is the *write half* of the
//! transport observed by the host. **No logging or tracing subscribers are
//! installed in client mode**, and callers are expected to avoid using
//! `print!`/`println!` directly. The default `log` and `tracing`
//! implementations are no-ops without an installed subscriber, so as long as
//! we don't initialize one we are guaranteed not to leak any text onto the
//! transport.
//!
//! # Why we don't use `tokio::io::stdout()` directly
//!
//! On at least some platforms (notably Windows), `std::io::Stdout` is line
//! buffered – it only flushes when it sees a `\n` byte or when explicitly
//! flushed. Even with explicit flush calls, going through the standard
//! library's stdout introduces avoidable buffering. The bridge protocol is
//! purely binary (length-prefixed frames), so any buffering in the write
//! path is undesirable.
//!
//! Instead, we use [`os_pipe`] to duplicate the OS-level stdin/stdout
//! handles, wrap them in [`tokio::fs::File`], and feed those to the
//! transport. Reads/writes go straight to the OS without any user-space
//! buffering layer in between.

use std::sync::Arc;

use bridge_rpc_core::{BridgeRpc, StreamTransport};
use bridge_rpc_router::Router;
use bridge_rpc_services::{
    RegisterServicesOptions, register_services_with_defaults,
};
use system_traits::impls::InMemorySys;
use system_traits::impls::RealSys;
use tokio::fs::File as TokioFile;

use crate::cli::ClientArgs;
use crate::error::{ErrorInner, Result};

/// Convenience type alias for the stdio transport we use in client mode.
///
/// Both halves are async [`TokioFile`]s wrapping freshly-duplicated
/// OS-level stdin/stdout handles, so they bypass `std::io::Stdin` /
/// `std::io::Stdout`'s internal buffering.
pub type StdioTransport = StreamTransport<TokioFile, TokioFile>;

/// Build a [`Router`] populated with every service from
/// [`bridge_rpc_services`], using the supplied `options` for path
/// configuration.
///
/// The router is backed by [`RealSys`] so that file-system and process
/// services interact with the actual host environment.
pub fn build_default_router(options: RegisterServicesOptions) -> Router {
    let mut router = Router::new();
    let sys = Arc::new(RealSys);
    register_services_with_defaults(&mut router, sys, options);
    router
}

/// Build a [`Router`] backed by [`InMemorySys`] – all FS operations are
/// performed against a fully in-memory virtual filesystem that is isolated
/// from the host operating system.
///
/// The in-memory system is initialised with its CWD set to `"/"` so that
/// proc services (e.g. `/proc/current-dir`) return a non-empty path.
pub fn build_inmemory_router(options: RegisterServicesOptions) -> Router {
    use system_traits::EnvSetCurrentDir as _;

    let sys = InMemorySys::default();
    // Give the in-memory process a sensible initial working directory so
    // that `/proc/current-dir` always returns a non-empty string.
    let _ = sys.env_set_current_dir(std::path::Path::new("/"));

    let mut router = Router::new();
    let sys = Arc::new(sys);
    register_services_with_defaults(&mut router, sys, options);
    router
}

/// Open a fresh, unbuffered handle to the process's stdin.
fn open_unbuffered_stdin() -> Result<TokioFile> {
    let pipe = os_pipe::dup_stdin().map_err(ErrorInner::from)?;
    let std_file = pipe_reader_into_file(pipe);
    Ok(TokioFile::from_std(std_file))
}

/// Open a fresh, unbuffered handle to the process's stdout.
fn open_unbuffered_stdout() -> Result<TokioFile> {
    let pipe = os_pipe::dup_stdout().map_err(ErrorInner::from)?;
    let std_file = pipe_writer_into_file(pipe);
    Ok(TokioFile::from_std(std_file))
}

#[cfg(unix)]
fn pipe_reader_into_file(pipe: os_pipe::PipeReader) -> std::fs::File {
    use std::os::unix::io::OwnedFd;
    let fd: OwnedFd = pipe.into();
    std::fs::File::from(fd)
}

#[cfg(unix)]
fn pipe_writer_into_file(pipe: os_pipe::PipeWriter) -> std::fs::File {
    use std::os::unix::io::OwnedFd;
    let fd: OwnedFd = pipe.into();
    std::fs::File::from(fd)
}

#[cfg(windows)]
fn pipe_reader_into_file(pipe: os_pipe::PipeReader) -> std::fs::File {
    use std::os::windows::io::OwnedHandle;
    let handle: OwnedHandle = pipe.into();
    std::fs::File::from(handle)
}

#[cfg(windows)]
fn pipe_writer_into_file(pipe: os_pipe::PipeWriter) -> std::fs::File {
    use std::os::windows::io::OwnedHandle;
    let handle: OwnedHandle = pipe.into();
    std::fs::File::from(handle)
}

/// Build a [`BridgeRpc`] wired to fresh duplicates of stdin/stdout and
/// the supplied `service`.
///
/// Returns an error if the OS handles for stdin or stdout cannot be
/// duplicated.
pub fn build_stdio_bridge<S>(service: S) -> Result<BridgeRpc<StdioTransport, S>>
where
    S: bridge_rpc_core::service::Service,
{
    let stdin = open_unbuffered_stdin()?;
    let stdout = open_unbuffered_stdout()?;
    let transport = StreamTransport::new(stdin, stdout);
    Ok(BridgeRpc::new(transport, service))
}

/// Run the bridge in client mode using the supplied [`ClientArgs`].
///
/// This call blocks until the host closes the bridge or the underlying
/// transport reports end-of-stream.
///
/// # Logging
///
/// This function intentionally does **not** install any logging or tracing
/// subscriber. Stdout is owned by the bridge transport, so any stray writes
/// would corrupt the framing. The default `log` and `tracing` global
/// dispatchers are no-ops, so log/tracing emissions from the dependent
/// crates will simply be discarded.
pub async fn run_client(args: ClientArgs) -> Result<()> {
    crate::tracing_setup::install_client_tracing()?;

    let router = match args.sys {
        crate::cli::SysKind::Real => {
            build_default_router(args.to_register_options())
        }
        crate::cli::SysKind::InMemory => {
            build_inmemory_router(args.to_register_options())
        }
    };
    let bridge = build_stdio_bridge(router)?;

    match bridge.run().await {
        Ok(()) => Ok(()),
        Err(err) if is_eof_transport_error(&err) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Returns true if `err` represents a clean transport-level end-of-stream
/// signal (the parent closed our stdin).
fn is_eof_transport_error(err: &bridge_rpc_core::BridgeRpcError) -> bool {
    use bridge_rpc_core::BridgeRpcErrorKind;
    if err.kind() != BridgeRpcErrorKind::Transport {
        return false;
    }
    // The transport variant wraps an `eyre::Report` whose message is
    // populated from the underlying `StreamTransportError`. EOS errors
    // stringify to exactly "end of stream".
    let msg = err.to_string();
    let chain = std::error::Error::source(err)
        .map(|s| s.to_string())
        .unwrap_or_default();
    msg.contains("end of stream") || chain.contains("end of stream")
}
