use std::collections::BTreeMap;

struct N;

impl std::fmt::Display for N {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl omni_notice::Notice for N {
    fn fields(&self) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::new()
    }
}

fn main() {}
