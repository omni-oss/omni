#![allow(clippy::redundant_field_names)]
#![feature(box_patterns)]

mod cache_configuration;
mod capabilities;
mod constants;
mod ignore;
mod meta_configuration;
mod owned_projection;
mod project_configuration;
mod projection;
mod remote_cache_configuration;
mod source;
mod task_configuration;
mod task_dependency_configuration;
mod task_extension;
mod task_output_configuration;
mod ui;
mod utils;
mod validators;
mod workspace_configuration;

pub use cache_configuration::*;
pub use capabilities::*;
pub use ignore::*;
pub use meta_configuration::*;
pub use omni_config_types as types;
pub use owned_projection::*;
pub use project_configuration::*;
pub use projection::*;
pub use remote_cache_configuration::*;
pub use source::*;
pub use task_configuration::*;
pub use task_dependency_configuration::*;
pub use task_extension::*;
pub use task_output_configuration::*;
pub use ui::*;
pub use utils::fs::{LoadConfigError, LoadConfigErrorKind};
pub use workspace_configuration::*;
