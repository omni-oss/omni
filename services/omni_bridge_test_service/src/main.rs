//! Binary entry point for `omni_bridge_test_service`.
//!
//! All heavy lifting lives in the library crate – this file just dispatches
//! to [`omni_bridge_test_service::main_entry`].

fn main() -> eyre::Result<()> {
    // The library returns its own [`Error`] type; flatten it into an
    // `eyre::Report` so we get a nicely-formatted error on stderr if the
    // process exits with a failure.
    omni_bridge_test_service::main_entry()
        .map_err(|e| eyre::eyre!("{e}"))
}
