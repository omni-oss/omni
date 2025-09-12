use std::{collections::HashMap, ffi::OsString};

use maps::Map;

pub type EnvVarsMap = Map<String, String>;
pub type EnvVarsOsMap = HashMap<OsString, OsString>;

pub(crate) fn vars_os(vars: &EnvVarsMap) -> EnvVarsOsMap {
    vars.iter()
        .map(|(k, v)| (k.clone().into(), v.clone().into()))
        .collect()
}
