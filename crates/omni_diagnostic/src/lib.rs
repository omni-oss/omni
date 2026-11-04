//! Structured diagnostics for CLI and MCP error reporting (RFC 0021).
//!
//! A [`Diagnostic`] is an ordinary error type that also describes itself:
//! stable code, [`Audience`], structured details and hints. Types opt in with
//! `#[derive(Diagnostic)]`, which also registers them so [`find`] can locate a
//! diagnostic anywhere inside an [`eyre::Report`] without knowing every type.

extern crate self as omni_diagnostic;

pub mod catalog;
mod registry;
pub mod render;
pub mod testing;

use std::{error::Error, fmt};

pub use omni_diagnostic_derive::Diagnostic;
#[cfg(feature = "explicit-registry")]
pub use registry::install;
pub use registry::{Registration, registrations};
pub use render::DiagnosticReport;

pub use omni_codes::Audience;

/// An error that describes itself to users and agents.
///
/// `Display` is the one-line summary. Bodies and hints are separate so each
/// surface can lay them out.
pub trait Diagnostic: Error + Send + Sync + 'static {
    /// Stable, dotted, lowercase identifier. Part of the public interface.
    fn code(&self) -> &'static str;

    /// Who is expected to act on this.
    fn audience(&self) -> Audience {
        Audience::User
    }

    /// Structured payload. `None` means "message only".
    fn details(&self) -> Option<serde_json::Value> {
        None
    }

    /// Actionable suggestions, rendered after the body.
    fn hints(&self) -> Vec<String> {
        Vec::new()
    }

    /// Extra text rendered between the summary and the hints (CLI only).
    fn render_body(&self, _out: &mut dyn fmt::Write) -> fmt::Result {
        Ok(())
    }
}

/// Per-variant metadata stored in a [`Registration`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeInfo {
    pub code: &'static str,
    pub audience: Audience,
}

/// Finds a diagnostic in a single error (not its sources).
pub fn find_in_error<'a>(
    error: &'a (dyn Error + 'static),
) -> Option<&'a dyn Diagnostic> {
    registrations().find_map(|r| (r.lookup)(error))
}

/// Finds the **outermost** diagnostic in a report's cause chain.
pub fn find(report: &eyre::Report) -> Option<&dyn Diagnostic> {
    report.chain().find_map(find_in_error)
}

#[doc(hidden)]
pub mod __private {
    pub use omni_codes;
    pub use serde_json;

    #[cfg(all(feature = "inventory", not(feature = "explicit-registry")))]
    pub use inventory;

    pub fn to_value<T: serde::Serialize + ?Sized>(v: &T) -> serde_json::Value {
        serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
    }
}

/// Registers a derived diagnostic type. Emitted by `#[derive(Diagnostic)]`.
#[cfg(all(feature = "inventory", not(feature = "explicit-registry")))]
#[macro_export]
macro_rules! register {
    ($t:ty) => {
        $crate::__private::inventory::submit! {
            <$t>::__DIAGNOSTIC_REGISTRATION
        }
    };
}

/// Explicit-registry backend: nothing is registered implicitly. Add
/// `<Type>::__DIAGNOSTIC_REGISTRATION` to the list given to `install`.
#[cfg(feature = "explicit-registry")]
#[macro_export]
macro_rules! register {
    ($t:ty) => {};
}

/// Builds a [`Registration`] for a *wrapper* error that hides a diagnostic.
///
/// `#[error(transparent)]` forwards `source()` past the inner error, so the
/// inner diagnostic never appears in the cause chain. A wrapper type exposes
/// it instead by giving a function that digs it out:
///
/// ```ignore
/// omni_diagnostic::register_wrapper!(TaskExecutorError, |e| {
///     e.meta_filter_error().map(|m| m as &dyn Diagnostic)
/// });
/// ```
#[macro_export]
macro_rules! wrapper_registration {
    ($t:ty, $f:expr) => {{
        fn __lookup<'a>(
            e: &'a (dyn ::std::error::Error + 'static),
        ) -> ::core::option::Option<&'a dyn $crate::Diagnostic> {
            let f: for<'b> fn(
                &'b $t,
            ) -> ::core::option::Option<
                &'b dyn $crate::Diagnostic,
            > = $f;
            e.downcast_ref::<$t>().and_then(f)
        }
        $crate::Registration {
            type_name: ::core::concat!(
                ::core::module_path!(),
                "::",
                ::core::stringify!($t)
            ),
            codes: &[],
            lookup: __lookup,
        }
    }};
}

/// Registers a wrapper error (see [`wrapper_registration!`]).
#[cfg(all(feature = "inventory", not(feature = "explicit-registry")))]
#[macro_export]
macro_rules! register_wrapper {
    ($t:ty, $f:expr) => {
        $crate::__private::inventory::submit! {
            $crate::wrapper_registration!($t, $f)
        }
    };
}

/// Explicit-registry backend: add `wrapper_registration!(..)` to the list
/// given to `install` instead.
#[cfg(feature = "explicit-registry")]
#[macro_export]
macro_rules! register_wrapper {
    ($t:ty, $f:expr) => {};
}

#[cfg(not(any(feature = "inventory", feature = "explicit-registry")))]
compile_error!(
    "omni_diagnostic needs the `inventory` or `explicit-registry` feature"
);
