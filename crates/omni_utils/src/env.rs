use std::{collections::HashMap, ffi::OsString};

pub fn to_vars_os(
    vars: &maps::Map<String, String>,
) -> HashMap<OsString, OsString> {
    vars.iter()
        .map(|(k, v)| (k.clone().into(), v.clone().into()))
        .collect()
}
