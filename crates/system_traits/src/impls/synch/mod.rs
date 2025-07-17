#[cfg(any(feature = "real-sync", feature = "memory-sync"))]
use crate::EnvVars;

#[cfg(feature = "real-sync")]
pub use sys_traits::impls::RealSys;

#[cfg(feature = "real-sync")]
impl EnvVars for RealSys {
    fn env_vars(&self) -> std::env::Vars {
        std::env::vars()
    }

    fn env_vars_os(&self) -> std::env::VarsOs {
        std::env::vars_os()
    }
}

#[cfg(feature = "memory-sync")]
pub use sys_traits::impls::InMemorySys;

#[cfg(feature = "memory-sync")]
impl EnvVars for InMemorySys {
    fn env_vars(&self) -> std::env::Vars {
        std::env::vars()
    }

    fn env_vars_os(&self) -> std::env::VarsOs {
        std::env::vars_os()
    }
}
