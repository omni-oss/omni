use std::ffi::OsString;

use maps::Map;

pub type EnvVarsMap = Map<String, String>;
pub type EnvVarsMapOs = Map<OsString, OsString>;
