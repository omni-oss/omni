use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Serialize `value` to [`rmpv::Value`] using **named-field (map) encoding**
/// for structs.
///
/// [`rmpv::ext::to_value`] delegates `serialize_struct` directly to
/// `serialize_tuple_struct`, which means every struct becomes a positional
/// [`rmpv::Value::Array`] – field names are silently dropped.  That compact
/// representation is incompatible with the JS `@msgpack/msgpack` library,
/// which decodes msgpack maps as plain objects and msgpack arrays as `Array`
/// instances.
///
/// This function serialises through [`rmp_serde::to_vec_named`] (which emits
/// structs as msgpack maps with string keys) and then reads the resulting
/// bytes back into a [`rmpv::Value`] via [`rmpv::decode::read_value`].  The
/// round-trip is cheap for the small payloads carried in bridge RPC headers.
///
/// # Errors
///
/// Returns [`rmpv::ext::Error::Syntax`] wrapping a human-readable message if
/// either the `rmp_serde` encode step or the `rmpv` decode step fails.
fn to_value_named<T: serde::Serialize>(
    value: &T,
) -> Result<rmpv::Value, rmpv::ext::Error> {
    // Encode with rmp_serde's named map format so struct fields become
    // string-keyed map entries rather than positional array elements.
    let bytes = rmp_serde::to_vec_named(value)
        .map_err(|e| rmpv::ext::Error::Syntax(e.to_string()))?;

    // Decode the raw msgpack bytes back into a rmpv::Value.
    rmpv::decode::read_value(&mut &bytes[..])
        .map_err(|e| rmpv::ext::Error::Syntax(e.to_string()))
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DynMap(HashMap<String, rmpv::Value>);

impl DynMap {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert_raw(
        &mut self,
        key: impl Into<String>,
        value: impl Into<rmpv::Value>,
    ) {
        self.0.insert(key.into(), value.into());
    }

    pub fn insert<T: serde::Serialize>(
        &mut self,
        key: impl Into<String>,
        value: T,
    ) -> Result<(), rmpv::ext::Error> {
        let rmpv = to_value_named(&value)?;
        self.insert_raw(key, rmpv);
        Ok(())
    }

    pub fn get<T: serde::de::DeserializeOwned>(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<T>, rmpv::ext::Error> {
        let item = self.get_raw(key);
        if let Some(value) = item {
            rmpv::ext::from_value(value.clone()).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn get_raw(&self, key: impl AsRef<str>) -> Option<&rmpv::Value> {
        self.0.get(key.as_ref())
    }

    pub fn has_key(&self, key: impl AsRef<str>) -> bool {
        self.0.contains_key(key.as_ref())
    }

    pub fn get_raw_mut(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
        count: u32,
    }

    #[test]
    fn new_creates_empty_map() {
        let map = DynMap::new();
        assert!(!map.has_key("anything"));
        assert!(map.get_raw("anything").is_none());
    }

    #[test]
    fn insert_raw_and_get_raw_round_trip() {
        let mut map = DynMap::new();
        map.insert_raw("key", rmpv::Value::String("value".into()));

        let value = map.get_raw("key").expect("key should exist");
        assert_eq!(value, &rmpv::Value::String("value".into()));
    }

    #[test]
    fn insert_raw_accepts_string_owned_keys() {
        let mut map = DynMap::new();
        map.insert_raw(String::from("owned_key"), 42i64);

        assert_eq!(
            map.get_raw("owned_key"),
            Some(&rmpv::Value::Integer(42.into()))
        );
    }

    #[test]
    fn insert_serializes_complex_values() {
        let mut map = DynMap::new();
        let sample = Sample {
            name: "alice".to_string(),
            count: 7,
        };

        map.insert("sample", sample.clone())
            .expect("insert should succeed");

        let retrieved: Sample = map
            .get("sample")
            .expect("deserialization should succeed")
            .expect("value should exist");

        assert_eq!(retrieved, sample);
    }

    #[test]
    fn insert_supports_primitives_and_vecs() {
        let mut map = DynMap::new();

        map.insert("int", 123i64).unwrap();
        map.insert("float", 3.5f64).unwrap();
        map.insert("bool", true).unwrap();
        map.insert("string", "hello".to_string()).unwrap();
        map.insert("vec", vec![1u32, 2, 3]).unwrap();

        assert_eq!(map.get::<i64>("int").unwrap(), Some(123));
        assert_eq!(map.get::<f64>("float").unwrap(), Some(3.5));
        assert_eq!(map.get::<bool>("bool").unwrap(), Some(true));
        assert_eq!(
            map.get::<String>("string").unwrap(),
            Some("hello".to_string())
        );
        assert_eq!(map.get::<Vec<u32>>("vec").unwrap(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let map = DynMap::new();
        let result: Option<String> =
            map.get("missing").expect("missing key should not error");
        assert!(result.is_none());
    }

    #[test]
    fn get_raw_returns_none_for_missing_key() {
        let map = DynMap::new();
        assert!(map.get_raw("missing").is_none());
    }

    #[test]
    fn get_returns_error_on_type_mismatch() {
        let mut map = DynMap::new();
        map.insert("key", "not a number".to_string()).unwrap();

        let result: Result<Option<i64>, _> = map.get("key");
        assert!(result.is_err(), "expected deserialization error");
    }

    #[test]
    fn has_key_reflects_presence() {
        let mut map = DynMap::new();
        assert!(!map.has_key("key"));

        map.insert_raw("key", 1i64);
        assert!(map.has_key("key"));
        assert!(!map.has_key("other"));
    }

    #[test]
    fn insert_overwrites_existing_value() {
        let mut map = DynMap::new();
        map.insert_raw("key", 1i64);
        map.insert_raw("key", 2i64);

        assert_eq!(map.get_raw("key"), Some(&rmpv::Value::Integer(2.into())));
    }

    #[test]
    fn get_raw_mut_allows_mutation() {
        let mut map = DynMap::new();
        map.insert_raw("key", 1i64);

        let value = map.get_raw_mut("key").expect("key should exist");
        *value = rmpv::Value::Integer(99.into());

        assert_eq!(map.get_raw("key"), Some(&rmpv::Value::Integer(99.into())));
    }

    #[test]
    fn get_raw_mut_returns_none_for_missing_key() {
        let mut map = DynMap::new();
        assert!(map.get_raw_mut("missing").is_none());
    }

    #[test]
    fn remove_returns_value_and_deletes_key() {
        let mut map = DynMap::new();
        map.insert_raw("key", 42i64);

        let removed = map.remove("key");
        assert_eq!(removed, Some(rmpv::Value::Integer(42.into())));
        assert!(!map.has_key("key"));
        assert!(map.get_raw("key").is_none());
    }

    #[test]
    fn remove_returns_none_for_missing_key() {
        let mut map = DynMap::new();
        assert!(map.remove("missing").is_none());
    }

    #[test]
    fn key_lookup_accepts_str_and_string() {
        let mut map = DynMap::new();
        map.insert_raw("key", 1i64);

        let owned_key = String::from("key");
        assert!(map.has_key("key"));
        assert!(map.has_key(&owned_key));
        assert!(map.has_key(owned_key.as_str()));
    }

    #[test]
    fn equality_compares_contents() {
        let mut a = DynMap::new();
        let mut b = DynMap::new();

        assert_eq!(a, b);

        a.insert_raw("k", 1i64);
        b.insert_raw("k", 1i64);
        assert_eq!(a, b);

        b.insert_raw("k", 2i64);
        assert_ne!(a, b);
    }

    #[test]
    fn clone_produces_independent_copy() {
        let mut original = DynMap::new();
        original.insert_raw("key", 1i64);

        let cloned = original.clone();
        original.insert_raw("key", 2i64);

        assert_eq!(
            cloned.get_raw("key"),
            Some(&rmpv::Value::Integer(1.into()))
        );
        assert_eq!(
            original.get_raw("key"),
            Some(&rmpv::Value::Integer(2.into()))
        );
    }

    #[test]
    fn serde_round_trips_via_msgpack() {
        let mut map = DynMap::new();
        map.insert_raw("a", 1i64);
        map.insert_raw("b", "hello".to_string());

        let bytes = rmp_serde::to_vec(&map).expect("serialize");
        let decoded: DynMap =
            rmp_serde::from_slice(&bytes).expect("deserialize");

        assert_eq!(map, decoded);
    }

    #[test]
    fn serde_is_transparent_over_inner_map() {
        // Because of `#[serde(transparent)]`, a DynMap should serialize
        // identically to its inner HashMap.
        let mut map = DynMap::new();
        map.insert_raw("key", 7i64);

        let mut inner: HashMap<String, rmpv::Value> = HashMap::new();
        inner.insert("key".to_string(), rmpv::Value::Integer(7.into()));

        let map_bytes = rmp_serde::to_vec(&map).expect("serialize map");
        let inner_bytes = rmp_serde::to_vec(&inner).expect("serialize inner");

        assert_eq!(map_bytes, inner_bytes);
    }

    #[test]
    fn insert_struct_produces_named_map_not_array() {
        // Regression test: DynMap::insert must use named-field (map) encoding
        // so that the JS `@msgpack/msgpack` decoder receives a plain object
        // with string keys rather than a positional array.
        let mut map = DynMap::new();
        let sample = Sample {
            name: "alice".to_string(),
            count: 7,
        };

        map.insert("sample", &sample)
            .expect("insert should succeed");

        let raw = map.get_raw("sample").expect("value should be stored");

        // Must be a Map, NOT an Array.
        assert!(
            matches!(raw, rmpv::Value::Map(_)),
            "struct should be stored as a named map; got {raw:?}"
        );
        assert!(
            !matches!(raw, rmpv::Value::Array(_)),
            "struct must not use positional/tuple array encoding; got {raw:?}"
        );
    }

    #[test]
    fn insert_struct_map_has_string_field_name_keys() {
        let mut map = DynMap::new();
        map.insert(
            "item",
            &Sample {
                name: "bob".to_string(),
                count: 3,
            },
        )
        .expect("insert should succeed");

        let raw = map.get_raw("item").expect("value should be stored");
        let rmpv::Value::Map(entries) = raw else {
            panic!("expected Map, got {raw:?}");
        };

        let has_name_key = entries.iter().any(|(k, v)| {
            k == &rmpv::Value::String("name".into())
                && *v == rmpv::Value::String("bob".into())
        });
        let has_count_key = entries.iter().any(|(k, v)| {
            k == &rmpv::Value::String("count".into())
                && *v == rmpv::Value::Integer(3.into())
        });

        assert!(has_name_key, "expected 'name' key with value 'bob'");
        assert!(has_count_key, "expected 'count' key with value 3");
    }

    #[test]
    fn to_value_named_produces_map_for_named_struct() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Point {
            x: f64,
            y: f64,
        }

        let v = to_value_named(&Point { x: 1.0, y: 2.0 })
            .expect("to_value_named should succeed");

        assert!(
            matches!(v, rmpv::Value::Map(_)),
            "Point should encode as a Map; got {v:?}"
        );
    }

    #[test]
    fn to_value_named_passes_through_primitives_unchanged() {
        assert_eq!(
            to_value_named(&42i64).unwrap(),
            rmpv::Value::Integer(42.into())
        );
        assert_eq!(to_value_named(&true).unwrap(), rmpv::Value::Boolean(true));
        assert_eq!(
            to_value_named(&"hello").unwrap(),
            rmpv::Value::String("hello".into())
        );
    }

    #[test]
    fn to_value_named_round_trips_nested_struct() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Outer {
            inner: Inner,
            tag: String,
        }
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Inner {
            value: u32,
        }

        let original = Outer {
            inner: Inner { value: 99 },
            tag: "test".to_string(),
        };

        let encoded = to_value_named(&original).expect("encode");
        let decoded: Outer = rmpv::ext::from_value(encoded).expect("decode");
        assert_eq!(decoded, original);
    }

    #[test]
    fn insert_and_get_round_trip_still_works_with_named_encoding() {
        // Ensure the change to named encoding does not break the Rust-side
        // round-trip via DynMap::insert + DynMap::get.
        let mut map = DynMap::new();
        let original = Sample {
            name: "carol".to_string(),
            count: 42,
        };

        map.insert("s", &original).expect("insert");

        let retrieved: Sample =
            map.get("s").expect("get ok").expect("value present");
        assert_eq!(retrieved, original);
    }

    #[test]
    fn headers_and_trailers_aliases_are_dyn_map() {
        let mut headers: Headers = Headers::new();
        headers.insert_raw("h", 1i64);
        let _: DynMap = headers;

        let mut trailers: Trailers = Trailers::new();
        trailers.insert_raw("t", 2i64);
        let _: DynMap = trailers;
    }
}
