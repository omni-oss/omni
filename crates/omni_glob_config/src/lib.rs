//! Structured include/exclude glob configuration.
//!
//! A config field is one of three forms: a bare pattern, a list of patterns, or
//! an object with `include` and `exclude` lists. The bare and list forms are
//! include-only. Exclusion is expressed only through the object form's
//! `exclude`, which always wins. Match order does not matter.
//!
//! [`GlobConfig`] merges last-writer-wins. [`MergeGlobConfig`] carries
//! [`config_utils::ListConfig`]'s `append`/`prepend`/`replace`/`merge` layering
//! on each side, for the fields that layer a project's entries onto workspace
//! defaults. Both normalize to an [`omni_glob::GlobPatterns`] the consumer
//! compiles.

use merge::Merge;
use omni_config_types::{MergeSingleOrMany, SingleOrMany};
use omni_glob::GlobPatterns;

/// A bare pattern, a list of patterns, or an explicit include/exclude object.
///
/// Merges last-writer-wins, so a later layer replaces an earlier one whole.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum GlobConfig<T> {
    Single(T),
    Many(Vec<T>),
    IncludeAndExclude {
        include: SingleOrMany<T>,
        exclude: SingleOrMany<T>,
    },
}

impl<T> GlobConfig<T> {
    pub fn normalize(self) -> GlobPatterns<T> {
        match self {
            Self::Single(t) => GlobPatterns {
                include: vec![t],
                exclude: Vec::new(),
            },
            Self::Many(v) => GlobPatterns {
                include: v,
                exclude: Vec::new(),
            },
            Self::IncludeAndExclude { include, exclude } => GlobPatterns {
                include: include.into_vec(),
                exclude: exclude.into_vec(),
            },
        }
    }
}

impl<T> Default for GlobConfig<T> {
    fn default() -> Self {
        Self::Many(Vec::new())
    }
}

impl<T> Merge for GlobConfig<T> {
    fn merge(&mut self, other: Self) {
        *self = other;
    }
}

/// The layering counterpart of [`GlobConfig`]. Each side keeps
/// [`config_utils::ListConfig`]'s `append`/`prepend`/`replace`/`merge` forms so
/// a project can extend a workspace default without restating it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum MergeGlobConfig<T: Merge> {
    Single(T),
    Many(MergeSingleOrMany<T>),
    IncludeAndExclude {
        include: MergeSingleOrMany<T>,
        exclude: MergeSingleOrMany<T>,
    },
}

impl<T: Merge> MergeGlobConfig<T> {
    pub fn normalize(self) -> GlobPatterns<T> {
        let (include, exclude) = self.into_sides();
        GlobPatterns {
            include: include.into_vec(),
            exclude: exclude.into_vec(),
        }
    }

    // Every form as an (include, exclude) pair. Single and Many have an empty
    // exclude, so a cross-variant merge is always a per-side merge.
    fn into_sides(self) -> (MergeSingleOrMany<T>, MergeSingleOrMany<T>) {
        match self {
            Self::Single(t) => {
                (MergeSingleOrMany::Single(t), MergeSingleOrMany::empty())
            }
            Self::Many(m) => (m, MergeSingleOrMany::empty()),
            Self::IncludeAndExclude { include, exclude } => (include, exclude),
        }
    }

    /// Mutable access to every pattern on both sides, for in-place resolution.
    pub fn iter_mut(&mut self) -> Box<dyn Iterator<Item = &mut T> + '_> {
        match self {
            Self::Single(t) => Box::new(std::slice::from_mut(t).iter_mut()),
            Self::Many(m) => Box::new(m.iter_mut()),
            Self::IncludeAndExclude { include, exclude } => {
                Box::new(include.iter_mut().chain(exclude.iter_mut()))
            }
        }
    }
}

impl<T: Merge> Default for MergeGlobConfig<T> {
    fn default() -> Self {
        Self::Many(MergeSingleOrMany::empty())
    }
}

// A cross-variant merge (a Many under an IncludeAndExclude) folds the Many into
// the include side, so the result is always a well-defined include/exclude
// pair.
impl<T: Merge + Clone> Merge for MergeGlobConfig<T> {
    fn merge(&mut self, other: Self) {
        let this =
            std::mem::replace(self, Self::Many(MergeSingleOrMany::empty()));
        let (mut si, mut se) = this.into_sides();
        let (oi, oe) = other.into_sides();
        si.merge(oi);
        se.merge(oe);
        *self = Self::IncludeAndExclude {
            include: si,
            exclude: se,
        };
    }
}

#[cfg(feature = "serde")]
mod de {
    use omni_config_types::{MergeSingleOrMany, SingleOrMany};
    use serde::de::DeserializeOwned;
    use serde_json::{Map, Value};

    use crate::{GlobConfig, MergeGlobConfig};

    fn reject_unknown_keys(obj: &Map<String, Value>) -> Result<(), String> {
        for key in obj.keys() {
            if key != "include" && key != "exclude" {
                return Err(format!(
                    "unknown field `{key}`, expected `include` or `exclude`"
                ));
            }
        }
        Ok(())
    }

    fn side<S>(
        obj: &Map<String, Value>,
        key: &str,
        empty: impl FnOnce() -> S,
    ) -> Result<S, String>
    where
        S: DeserializeOwned,
    {
        match obj.get(key) {
            Some(v) => serde_path_to_error::deserialize(v.clone())
                .map_err(|e| e.to_string()),
            None => Ok(empty()),
        }
    }

    fn glob_config_from_object<T: DeserializeOwned>(
        value: Value,
    ) -> Result<GlobConfig<T>, String> {
        let obj = value
            .as_object()
            .ok_or_else(|| "expected a mapping".to_string())?;
        reject_unknown_keys(obj)?;
        Ok(GlobConfig::IncludeAndExclude {
            include: side(obj, "include", || SingleOrMany::Many(Vec::new()))?,
            exclude: side(obj, "exclude", || SingleOrMany::Many(Vec::new()))?,
        })
    }

    fn merge_glob_config_from_object<T: DeserializeOwned + merge::Merge>(
        value: Value,
    ) -> Result<MergeGlobConfig<T>, String> {
        let obj = value
            .as_object()
            .ok_or_else(|| "expected a mapping".to_string())?;

        // An include/exclude object is IncludeAndExclude. Any other object is a
        // ListConfig layering form (append, prepend, merge, replace), handled
        // by MergeSingleOrMany.
        if obj.contains_key("include") || obj.contains_key("exclude") {
            reject_unknown_keys(obj)?;
            Ok(MergeGlobConfig::IncludeAndExclude {
                include: side(obj, "include", MergeSingleOrMany::empty)?,
                exclude: side(obj, "exclude", MergeSingleOrMany::empty)?,
            })
        } else {
            serde_path_to_error::deserialize::<_, MergeSingleOrMany<T>>(value)
                .map(MergeGlobConfig::Many)
                .map_err(|e| e.to_string())
        }
    }

    impl<'de, T> serde::Deserialize<'de> for GlobConfig<T>
    where
        T: DeserializeOwned,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            serde_untagged::UntaggedEnumVisitor::new()
                .string(|s| {
                    serde_json::from_value(Value::String(s.to_owned()))
                        .map(GlobConfig::Single)
                        .map_err(serde::de::Error::custom)
                })
                .seq(|s| s.deserialize().map(GlobConfig::Many))
                .map(|m| {
                    let value: Value = m.deserialize()?;
                    glob_config_from_object(value)
                        .map_err(serde::de::Error::custom)
                })
                .deserialize(deserializer)
        }
    }

    impl<'de, T> serde::Deserialize<'de> for MergeGlobConfig<T>
    where
        T: DeserializeOwned + merge::Merge,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            serde_untagged::UntaggedEnumVisitor::new()
                .string(|s| {
                    serde_json::from_value(Value::String(s.to_owned()))
                        .map(MergeGlobConfig::Single)
                        .map_err(serde::de::Error::custom)
                })
                .seq(|s| {
                    s.deserialize::<Vec<T>>().map(|v| {
                        MergeGlobConfig::Many(MergeSingleOrMany::from_vec(v))
                    })
                })
                .map(|m| {
                    let value: Value = m.deserialize()?;
                    merge_glob_config_from_object(value)
                        .map_err(serde::de::Error::custom)
                })
                .deserialize(deserializer)
        }
    }
}

#[cfg(test)]
mod tests {
    use config_utils::ListConfig;
    use merge::Merge;
    use omni_config_types::{MergeSingleOrMany, SingleOrMany};

    use super::*;

    #[test]
    fn glob_config_scalar_normalizes_to_include_only() {
        let cfg: GlobConfig<String> =
            serde_json::from_str(r#""src/**""#).unwrap();
        let patterns = cfg.normalize();
        assert_eq!(patterns.include, vec!["src/**".to_string()]);
        assert!(patterns.exclude.is_empty());
    }

    #[test]
    fn glob_config_list_normalizes_to_include_only() {
        let cfg: GlobConfig<String> =
            serde_json::from_str(r#"["a/**", "b/**"]"#).unwrap();
        let patterns = cfg.normalize();
        assert_eq!(
            patterns.include,
            vec!["a/**".to_string(), "b/**".to_string()]
        );
        assert!(patterns.exclude.is_empty());
    }

    #[test]
    fn glob_config_include_exclude_splits_both_sides() {
        let cfg: GlobConfig<String> = serde_json::from_str(
            r#"{"include": ["src/**"], "exclude": "src/gen/**"}"#,
        )
        .unwrap();
        assert_eq!(
            cfg,
            GlobConfig::IncludeAndExclude {
                include: SingleOrMany::Many(vec!["src/**".to_string()]),
                exclude: SingleOrMany::Single("src/gen/**".to_string()),
            }
        );
        let patterns = cfg.normalize();
        assert_eq!(patterns.include, vec!["src/**".to_string()]);
        assert_eq!(patterns.exclude, vec!["src/gen/**".to_string()]);
    }

    #[test]
    fn glob_config_missing_side_defaults_to_empty() {
        let cfg: GlobConfig<String> =
            serde_json::from_str(r#"{"exclude": ["gen/**"]}"#).unwrap();
        let patterns = cfg.normalize();
        assert!(patterns.include.is_empty());
        assert_eq!(patterns.exclude, vec!["gen/**".to_string()]);
    }

    #[test]
    fn glob_config_rejects_unknown_object_key() {
        let err = serde_json::from_str::<GlobConfig<String>>(
            r#"{"include": ["src/**"], "nope": ["x"]}"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("unknown field `nope`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn glob_config_merge_is_last_writer_wins() {
        let mut base = GlobConfig::Many(vec!["a/**".to_string()]);
        base.merge(GlobConfig::Single("b/**".to_string()));
        assert_eq!(base, GlobConfig::Single("b/**".to_string()));
    }

    #[test]
    fn glob_config_round_trips_include_exclude() {
        let cfg = GlobConfig::IncludeAndExclude {
            include: SingleOrMany::Many(vec!["src/**".to_string()]),
            exclude: SingleOrMany::Many(vec!["src/gen/**".to_string()]),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: GlobConfig<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn merge_glob_config_scalar_and_list_are_include_only() {
        let scalar: MergeGlobConfig<config_utils::Replace<String>> =
            serde_json::from_str(r#""src/**""#).unwrap();
        assert!(scalar.normalize().exclude.is_empty());

        let list: MergeGlobConfig<config_utils::Replace<String>> =
            serde_json::from_str(r#"["a/**", "b/**"]"#).unwrap();
        let patterns = list.normalize();
        assert_eq!(patterns.include.len(), 2);
        assert!(patterns.exclude.is_empty());
    }

    #[test]
    fn merge_glob_config_append_form_parses_to_many() {
        let cfg: MergeGlobConfig<config_utils::Replace<String>> =
            serde_json::from_str(r#"{"append": ["a/**"]}"#).unwrap();
        assert_eq!(
            cfg,
            MergeGlobConfig::Many(MergeSingleOrMany::List(ListConfig::append(
                vec![config_utils::Replace::new("a/**".to_string())]
            )))
        );
    }

    #[test]
    fn merge_glob_config_layers_each_side() {
        let mut base = MergeGlobConfig::IncludeAndExclude {
            include: MergeSingleOrMany::from_vec(vec![
                config_utils::Replace::new("src/**".to_string()),
            ]),
            exclude: MergeSingleOrMany::empty(),
        };
        let overlay = MergeGlobConfig::IncludeAndExclude {
            include: MergeSingleOrMany::List(ListConfig::append(vec![
                config_utils::Replace::new("gen/**".to_string()),
            ])),
            exclude: MergeSingleOrMany::from_vec(vec![
                config_utils::Replace::new("dist/**".to_string()),
            ]),
        };

        base.merge(overlay);
        let patterns = base.normalize();
        assert_eq!(
            patterns.include,
            vec![
                config_utils::Replace::new("src/**".to_string()),
                config_utils::Replace::new("gen/**".to_string()),
            ]
        );
        assert_eq!(
            patterns.exclude,
            vec![config_utils::Replace::new("dist/**".to_string())]
        );
    }

    #[test]
    fn merge_glob_config_folds_cross_variant_many_into_include() {
        let mut base =
            MergeGlobConfig::Many(MergeSingleOrMany::from_vec(vec![
                config_utils::Replace::new("src/**".to_string()),
            ]));
        let overlay = MergeGlobConfig::IncludeAndExclude {
            include: MergeSingleOrMany::List(ListConfig::append(vec![
                config_utils::Replace::new("gen/**".to_string()),
            ])),
            exclude: MergeSingleOrMany::from_vec(vec![
                config_utils::Replace::new("dist/**".to_string()),
            ]),
        };

        base.merge(overlay);
        let patterns = base.normalize();
        assert_eq!(
            patterns.include,
            vec![
                config_utils::Replace::new("src/**".to_string()),
                config_utils::Replace::new("gen/**".to_string()),
            ]
        );
        assert_eq!(
            patterns.exclude,
            vec![config_utils::Replace::new("dist/**".to_string())]
        );
    }
}
