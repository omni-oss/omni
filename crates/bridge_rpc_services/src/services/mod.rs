pub mod common;
pub mod fs;
pub mod log;
pub mod proc;
pub mod register;

pub use register::{
    DEFAULT_FS_PREFIX, DEFAULT_LOG_PATH, DEFAULT_PROC_PREFIX, FsSys, ProcSys,
    RegisterServicesOptions, fs_routes, proc_routes, register_fs_services,
    register_log_service, register_proc_services, register_services,
    register_services_with_defaults,
};

#[cfg(test)]
mod test_harness;
