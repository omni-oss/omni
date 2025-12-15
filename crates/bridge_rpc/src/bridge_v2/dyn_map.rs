use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DynMap(HashMap<String, rmpv::Value>);

impl DynMap {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<rmpv::Value>,
    ) {
        self.0.insert(key.into(), value.into());
    }

    pub fn get(&self, key: impl AsRef<str>) -> Option<&rmpv::Value> {
        self.0.get(key.as_ref())
    }

    pub fn get_mut(
        &mut self,
        key: impl AsRef<str>,
    ) -> Option<&mut rmpv::Value> {
        self.0.get_mut(key.as_ref())
    }

    pub fn remove(&mut self, key: impl AsRef<str>) -> Option<rmpv::Value> {
        self.0.remove(key.as_ref())
    }
}

pub type Headers = DynMap;
pub type Trailers = DynMap;
