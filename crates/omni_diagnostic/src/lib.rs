//! Structured diagnostics for CLI and MCP error reporting (RFC 0021).
//!
//! A [`Diagnostic`] is an ordinary error type that also describes itself:
//! stable code, [`Audience`], structured details and hints. Types opt in with
//! `#[derive(Diagnostic)]`, which also registers them so [`find`] can locate a
//! diagnostic anywhere inside an [`eyre::Report`] without knowing every type.

extern crate self as omni_diagnostic;

pub mod catalog;
mod redacted;
mod registry;
pub mod render;
pub mod testing;

use std::{error::Error, fmt};

pub use omni_diagnostic_derive::Diagnostic;
pub use redacted::{RedactedDiagnostic, redact_report};
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

    /// Whether this value actually carries a diagnostic.
    ///
    /// Always `true` unless the type has `#[diagnostic(skip)]` variants,
    /// `delegate` variants whose inner error has no diagnostic, or `forward`
    /// variants whose inner value is itself not reportable. Lookup ignores
    /// values that are not reportable, so such values stay plain errors.
    ///
    /// The other methods return neutral values for a non-reportable value
    /// (`code()` is `""`). Check this before calling them on a value obtained
    /// any other way than lookup, including through forwarding wrappers.
    fn reportable(&self) -> bool {
        true
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
    // A redacted diagnostic has a dynamic code, so it is not registered.
    if let Some(redacted) = error.downcast_ref::<RedactedDiagnostic>() {
        return Some(redacted);
    }
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

    /// Autoref specialization behind `#[diagnostic(delegate)]`: an
    /// `eyre::Report` field is searched along its whole cause chain, a
    /// `Box<T>` (or `Box<dyn Error>`) field is looked up through the box, and
    /// any other error field is looked up as a single error. [`ViaReport`] and
    /// [`ViaBox`] win for their field types because they match one autoref
    /// step earlier.
    pub struct Delegate<'a, T>(pub &'a T);

    pub trait ViaReport<'a> {
        fn __omni_lookup(&self) -> Option<&'a dyn crate::Diagnostic>;
    }

    impl<'a> ViaReport<'a> for Delegate<'a, eyre::Report> {
        fn __omni_lookup(&self) -> Option<&'a dyn crate::Diagnostic> {
            crate::find(self.0)
        }
    }

    /// A boxed error is looked up through the box: the registered lookups
    /// downcast to the concrete type, and `Box<T>` is not `T`.
    pub trait ViaBox<'a> {
        fn __omni_lookup(&self) -> Option<&'a dyn crate::Diagnostic>;
    }

    impl<'a, T: std::error::Error + 'static> ViaBox<'a> for Delegate<'a, Box<T>> {
        fn __omni_lookup(&self) -> Option<&'a dyn crate::Diagnostic> {
            let inner: &'a T = &**self.0;
            crate::find_in_error(inner)
        }
    }

    // The type-erased boxes: `dyn Error` is unsized, so the impl above does
    // not cover them. They cannot overlap with it, which needs a sized `T`.
    macro_rules! via_erased_box {
        ($($ty:ty),+ $(,)?) => {$(
            impl<'a> ViaBox<'a> for Delegate<'a, Box<$ty>> {
                fn __omni_lookup(&self) -> Option<&'a dyn crate::Diagnostic> {
                    let inner: &'a (dyn std::error::Error + 'static) = &**self.0;
                    crate::find_in_error(inner)
                }
            }
        )+};
    }

    via_erased_box!(
        dyn std::error::Error,
        dyn std::error::Error + Send,
        dyn std::error::Error + Send + Sync,
    );

    pub trait ViaError<'a> {
        fn __omni_lookup(&self) -> Option<&'a dyn crate::Diagnostic>;
    }

    impl<'a, T: std::error::Error + 'static> ViaError<'a> for &Delegate<'a, T> {
        fn __omni_lookup(&self) -> Option<&'a dyn crate::Diagnostic> {
            crate::find_in_error(self.0)
        }
    }
}

/// Looks for a diagnostic inside a field. Emitted by `#[diagnostic(delegate)]`.
#[doc(hidden)]
#[macro_export]
macro_rules! __delegate_lookup {
    ($field:expr) => {{
        #[allow(unused_imports)]
        use $crate::__private::{ViaBox as _, ViaError as _, ViaReport as _};
        (&$crate::__private::Delegate($field)).__omni_lookup()
    }};
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
