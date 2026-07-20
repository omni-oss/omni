//! [`SpawnPolicy`]: the neutral, additive description of how a runtime process
//! should be launched so that it is confined to the policy.
//!
//! It is deliberately data-only (no closures, no OS handles) so it can be
//! inspected, logged, tested, and composed across backends before anything is
//! spawned. Today it carries pre-spawn command-line arguments and an optional
//! [`OsSandboxSpec`] (the data an OS sandbox backend contributes); the type is
//! the seam through which future backends (WASI preopens, sandbox descriptors)
//! can contribute without changing callers.

use std::path::PathBuf;

/// A platform-neutral description of the filesystem confinement a
/// [`Tier::OsSandbox`](crate::Tier::OsSandbox) backend wants applied to the
/// spawned process: the path subtrees it may read, and those it may read+write.
///
/// It is data-only on purpose — the actual kernel ruleset (e.g. Landlock) is
/// installed by the *spawner* from this description via
/// [`install_os_sandbox`](crate::install_os_sandbox), keeping [`SpawnPolicy`]
/// free of OS handles and inspectable ahead of time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsSandboxSpec {
    /// Absolute path subtrees the process may read (recursively).
    pub read_paths: Vec<PathBuf>,
    /// Absolute path subtrees the process may read *and* write (recursively).
    pub write_paths: Vec<PathBuf>,
    /// Literal program names the confined process is permitted to spawn. Their
    /// binary directories must be granted read/execute so the child can be
    /// `execve`'d under the sandbox; resolving each name to a concrete path is
    /// left to the *spawner* (which knows the ambient `PATH`), not this
    /// policy-translation layer. A program in a directory the sandbox already
    /// grants (e.g. `/usr/bin`) needs no extra entry, but naming it is harmless.
    pub exec_programs: Vec<String>,

    /// Concrete TCP ports the confined process is permitted to *connect* to
    /// (outbound). This is the port-only floor an OS sandbox (Landlock ABI V4+)
    /// can enforce for the `net` domain: a `host:port` capability lowers to its
    /// `port` here, so a script reaching a socket directly still cannot connect
    /// to a port the policy never allowed.
    ///
    /// It is deliberately *connect*-only and *port*-only. The `net` domain
    /// governs outbound access, so binding/listening is left unrestricted (an
    /// ephemeral-port server keeps working); and because the kernel cannot match
    /// a host, `host`-level enforcement still rests on the in-process shim (this
    /// is why the OS sandbox does not *claim* `net` coverage — the floor is
    /// partial). All-ports (`host:*`) and `deny` rules are not representable as a
    /// port allow-list and are never lowered here.
    pub connect_ports: Vec<u16>,
}

impl OsSandboxSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.read_paths.is_empty()
            && self.write_paths.is_empty()
            && self.exec_programs.is_empty()
            && self.connect_ports.is_empty()
    }

    /// Fold another spec's paths into this one (order preserved).
    pub fn extend(&mut self, other: OsSandboxSpec) {
        self.read_paths.extend(other.read_paths);
        self.write_paths.extend(other.write_paths);
        self.exec_programs.extend(other.exec_programs);
        self.connect_ports.extend(other.connect_ports);
    }
}

/// The accumulated launch restrictions contributed by one or more backends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpawnPolicy {
    /// Extra command-line arguments to pass to the runtime, in a deterministic
    /// order. For the Deno backend these are the `--allow-*` / `--deny-*` flags
    /// that replace today's blanket `--allow-all`.
    pub args: Vec<String>,

    /// Non-fatal diagnostics describing translation decisions (e.g. a domain
    /// that ended up fully denied because the policy granted nothing). Useful
    /// for "show why" output; never affects the decision.
    pub notes: Vec<String>,

    /// The OS-sandbox confinement to install on the child process, if any
    /// backend contributed one. `None` means no OS sandbox tier is active.
    pub os_sandbox: Option<OsSandboxSpec>,
}

impl SpawnPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn push_arg(&mut self, arg: impl Into<String>) {
        self.args.push(arg.into());
    }

    pub fn push_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    /// Fold another backend's contribution into this one (order preserved).
    pub fn extend(&mut self, other: SpawnPolicy) {
        self.args.extend(other.args);
        self.notes.extend(other.notes);
        match (self.os_sandbox.as_mut(), other.os_sandbox) {
            (_, None) => {}
            (None, some) => self.os_sandbox = some,
            (Some(existing), Some(incoming)) => existing.extend(incoming),
        }
    }
}
