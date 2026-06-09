//! Tracing setup for host mode.
//!
//! The host writes human-readable log output to **stderr** so that it does
//! not interfere with anything the host itself might want to print to
//! stdout. This module installs a small `tracing` subscriber that writes to
//! stderr and bridges the `log` crate into `tracing` so that log records
//! emitted by the bridge dependencies are observable.
//!
//! Client mode deliberately calls *none* of these helpers – stdout in the
//! client is the bridge transport, and any logging would corrupt the
//! framing.

use std::sync::Once;

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Parse a level name (e.g. `"info"`, `"trace"`, ...) into a
/// [`tracing_subscriber::filter::LevelFilter`].
fn parse_level(s: &str) -> eyre::Result<LevelFilter> {
    Ok(match s.trim().to_ascii_lowercase().as_str() {
        "off" => LevelFilter::OFF,
        "error" => LevelFilter::ERROR,
        "warn" | "warning" => LevelFilter::WARN,
        "info" => LevelFilter::INFO,
        "debug" => LevelFilter::DEBUG,
        "trace" => LevelFilter::TRACE,
        other => return Err(eyre::eyre!("unknown log level: {other}")),
    })
}

/// Install the host-side tracing subscriber.
///
/// This is idempotent: calling it more than once is a no-op (the second
/// call simply leaves the existing subscriber in place).
pub fn install_host_tracing(level: &str) -> eyre::Result<()> {
    static INIT: Once = Once::new();

    let level_filter = parse_level(level)?;

    let mut error: Option<eyre::Report> = None;

    INIT.call_once(|| {
        // Forward `log` records into `tracing` so we can centrally apply
        // filtering and formatting.
        if let Err(e) = tracing_log::LogTracer::init() {
            error = Some(eyre::eyre!(
                "failed to install log -> tracing bridge: {e}"
            ));
            return;
        }

        let fmt_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(false)
            .with_ansi(atty::is(atty::Stream::Stderr))
            .compact();

        let subscriber = tracing_subscriber::registry()
            .with(level_filter)
            .with(fmt_layer);

        if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
            error = Some(eyre::eyre!(
                "failed to set tracing subscriber: {e}"
            ));
        }
    });

    if let Some(error) = error {
        return Err(error);
    }

    Ok(())
}
