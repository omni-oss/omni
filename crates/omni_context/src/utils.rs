use std::{collections::HashMap, ffi::OsString};

use maps::Map;

pub type EnvVarsMap = Map<String, String>;

pub(crate) fn vars_os(
    vars: &Map<String, String>,
) -> HashMap<OsString, OsString> {
    vars.iter()
        .map(|(k, v)| (k.clone().into(), v.clone().into()))
        .collect()
}
