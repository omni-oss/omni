#![feature(decl_macro)]

#[doc(hidden)]
pub use tracing as _trace_impl;
pub use tracing::Level;
pub use tracing::instrument;

#[cfg(feature = "enabled")]
pub macro event($($tt:tt)+) {
    $crate::_trace_impl::event!($($tt)+);
}

#[cfg(not(feature = "enabled"))]
pub macro event($($tt:tt)+) {
    ()
}

pub macro trace($($tt:tt)+) {
    $crate::event!(Level::TRACE, $($tt)+)
}

pub macro debug($($tt:tt)+) {
    $crate::event!(Level::DEBUG, $($tt)+)
}

pub macro info($($tt:tt)+) {
    $crate::event!(Level::INFO, $($tt)+)
}

pub macro warn($($tt:tt)+) {
    $crate::event!(Level::WARN, $($tt)+)
}

pub macro error($($tt:tt)+) {
    $crate::event!(Level::ERROR, $($tt)+)
}

#[cfg(feature = "enabled")]
pub macro span($($tt:tt)+) {
    $crate::_trace_impl::span!($($tt)+);
}

#[cfg(not(feature = "enabled"))]
pub macro span($($tt:tt)+) {
    ()
}

pub macro trace_span($($tt:tt)+) {
    $crate::span!(Level::TRACE, $($tt)+)
}

pub macro debug_span($($tt:tt)+) {
    $crate::span!(Level::DEBUG, $($tt)+)
}

pub macro info_span($($tt:tt)+) {
    $crate::span!(Level::INFO, $($tt)+)
}

pub macro warn_span($($tt:tt)+) {
    $crate::span!(Level::WARN, $($tt)+)
}

pub macro error_span($($tt:tt)+) {
    $crate::span!(Level::ERROR, $($tt)+)
}

#[cfg(feature = "enabled")]
pub macro enabled {
    ($lvl:expr, $($field:tt)*) => {
        $crate::_trace_impl::enabled!($lvl, $($field)*)
    },

    ($lvl:expr) => {
        $crate::_trace_impl::enabled!($lvl, {})
    }
}

#[cfg(not(feature = "enabled"))]
pub macro enabled {
    ($lvl:expr, $($field:tt)*) => {
        false
    },

    ($lvl:expr) => {
        $crate::_trace_impl::enabled!($lvl, {})
    }
}

#[cfg(feature = "enabled")]
pub macro event_enabled {
    ($lvl:expr, $($field:tt)*) =>{
        $crate::_trace_impl::event_enabled!($lvl, $($field)*)
    },
    ($lvl:expr) => {
        $crate::_trace_impl::event_enabled!($lvl, {})
    }
}

#[cfg(not(feature = "enabled"))]
pub macro event_enabled{
    ($lvl:expr, $($field:tt)*) {
        false
    },
    ($lvl:expr) => {
        $crate::_trace_impl::event_enabled!($lvl, {})
    }
}

#[cfg(feature = "enabled")]
pub macro span_enabled{
    ($lvl:expr, $($field:tt)*) => {
        $crate::_trace_impl::span_enabled!($lvl, $($field)*)
    },
    ($lvl:expr) => {
        $crate::_trace_impl::span_enabled!($lvl, {})
    }
}

#[cfg(not(feature = "enabled"))]
pub macro span_enabled {
    ($lvl:expr, $($field:tt)*) {
        false
    },
    ($lvl:expr) => {
        $crate::_trace_impl::span_enabled!($lvl, {})
    }
}

/// Conditionally compiles a series of statements if tracing is enabled via feature gate
///
/// **Note that the statements are not scoped inside a block and are exposed to the subsequent
/// code if variables are declared.**
#[cfg(feature = "enabled")]
pub macro if_enabled {
    ($($s:stmt;)*) => {
        $(
            $s;
        )*
    },
}

#[cfg(feature = "enabled")]
pub macro if_not_enabled {
    ($($s:stmt)*) => {
    },
}

#[cfg(not(feature = "enabled"))]
pub macro if_not_enabled {
    ($($s:stmt;)*) => {
        $(
            $s;
        )*
    },
}

/// Conditionally compiles a series of statements if tracing is enabled via feature gate
///
/// **Note that the statements are not scoped inside a block and are exposed to the subsequent
/// code if variables are declared.**
#[cfg(not(feature = "enabled"))]
pub macro if_enabled {
    ($($s:stmt)*) => {
    },
}
