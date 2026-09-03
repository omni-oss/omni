use config_utils::{ListConfig, merge::Merge};

/// A bare scalar or any [`ListConfig`] form.
///
/// The list and the `append`/`prepend`/`replace`/`merge` layering forms are
/// [`ListConfig`]'s. This adds only the bare-scalar shorthand, which normalizes
/// to a one-element list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum MergeSingleOrMany<T: Merge> {
    Single(T),
    List(ListConfig<T>),
}

impl<T: Merge> MergeSingleOrMany<T> {
    pub fn empty() -> Self {
        Self::List(ListConfig::value(Vec::new()))
    }

    pub fn into_vec(self) -> Vec<T> {
        self.into_list().into_vec()
    }

    fn into_list(self) -> ListConfig<T> {
        match self {
            Self::Single(t) => ListConfig::value(vec![t]),
            Self::List(l) => l,
        }
    }
}

impl<T: Merge> Default for MergeSingleOrMany<T> {
    fn default() -> Self {
        Self::empty()
    }
}

// A scalar is `Single`; a sequence or object is handed to `ListConfig`, which
// already parses the list and every layering form. Hand-written (like
// `ListConfig`) so the error carries the failing key path.
#[cfg(feature = "serde")]
impl<'de, T> serde::Deserialize<'de> for MergeSingleOrMany<T>
where
    T: serde::de::DeserializeOwned + Merge,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde_untagged::UntaggedEnumVisitor::new()
            .string(|s| {
                serde_json::from_value(serde_json::Value::String(s.to_owned()))
                    .map(MergeSingleOrMany::Single)
                    .map_err(serde::de::Error::custom)
            })
            .seq(|s| s.deserialize().map(MergeSingleOrMany::List))
            .map(|m| m.deserialize().map(MergeSingleOrMany::List))
            .deserialize(deserializer)
    }
}

// `Single` normalizes to a one-element list, then the two lists merge with
// `ListConfig`'s append/prepend/replace/merge rules.
impl<T: Merge + Clone> Merge for MergeSingleOrMany<T> {
    fn merge(&mut self, other: Self) {
        let mut this = std::mem::replace(self, Self::empty()).into_list();
        this.merge(other.into_list());
        *self = Self::List(this);
    }
}

#[cfg(test)]
mod tests {
    use config_utils::Replace;

    use super::*;

    type Patterns = MergeSingleOrMany<Replace<String>>;

    fn replace(s: &str) -> Replace<String> {
        Replace::new(s.to_owned())
    }

    #[test]
    fn scalar_deserializes_to_single() {
        let parsed: Patterns = serde_json::from_str(r#""src/**""#).unwrap();
        assert_eq!(parsed, MergeSingleOrMany::Single(replace("src/**")));
    }

    #[test]
    fn sequence_deserializes_to_list_value() {
        let parsed: Patterns =
            serde_json::from_str(r#"["a/**", "b/**"]"#).unwrap();
        assert_eq!(
            parsed,
            MergeSingleOrMany::List(ListConfig::value(vec![
                replace("a/**"),
                replace("b/**"),
            ]))
        );
    }

    #[test]
    fn append_object_deserializes_to_list_append() {
        let parsed: Patterns =
            serde_json::from_str(r#"{"append": ["a/**"]}"#).unwrap();
        assert_eq!(
            parsed,
            MergeSingleOrMany::List(ListConfig::append(vec![replace("a/**")]))
        );
    }

    #[test]
    fn scalar_round_trips_through_serialize() {
        let value = MergeSingleOrMany::Single(replace("src/**"));
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, r#""src/**""#);
        let back: Patterns = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn merge_layers_append_onto_a_default() {
        let mut base =
            MergeSingleOrMany::List(ListConfig::value(vec![replace("src/**")]));
        let overlay =
            MergeSingleOrMany::List(ListConfig::append(vec![replace(
                "gen/**",
            )]));

        base.merge(overlay);

        assert_eq!(base.into_vec(), vec![replace("src/**"), replace("gen/**")]);
    }

    #[test]
    fn merge_normalizes_single_before_layering() {
        let mut base = MergeSingleOrMany::Single(replace("src/**"));
        let overlay =
            MergeSingleOrMany::List(ListConfig::append(vec![replace(
                "gen/**",
            )]));

        base.merge(overlay);

        assert_eq!(base.into_vec(), vec![replace("src/**"), replace("gen/**")]);
    }
}
