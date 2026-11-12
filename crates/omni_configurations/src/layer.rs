//! Presence-aware configuration layers.
//!
//! A layer mirrors a configuration file as written: every field is optional,
//! so an omitted field is distinguishable from one set to its default, and the
//! original `append`/`prepend`/`replace`/`merge` operators are kept for replay.
//! Layers apply no defaults and enforce no required fields (such as the project
//! name); that happens when composed layers are finalized.
//!
//! Merging layers follows the same per-field rules as the eager types, with an
//! omitted field contributing nothing. An explicit `null` deserializes as
//! omitted. `base` is the exception: it is a per-declaration marker, so the
//! later layer's value always replaces the earlier one, as for
//! [`crate::ProjectConfiguration`].

use config_utils::{DictConfig, DynValue, ListConfig, Replace};
use merge::Merge;
use omni_command_config::CommandConfig;
use omni_config_types::{SingleOrMany, TeraExprBoolean};
use omni_glob_config::MergeGlobConfig;
use omni_retry_config::RetryConfiguration;
use omni_task_output_logs::OutputLogsConfiguration;
use omni_types::OmniPath;
use serde::{Deserialize, Serialize};

use crate::{
    MetaConfiguration, RedactConfiguration, TaskDependencyConfiguration,
    TaskEnvConfiguration,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge)]
#[serde(deny_unknown_fields)]
pub struct TaskOutputLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub files: Option<MergeGlobConfig<OmniPath>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub logs: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge)]
#[serde(deny_unknown_fields)]
pub struct CacheKeyLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub defaults: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub env: Option<ListConfig<Replace<String>>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub files: Option<MergeGlobConfig<OmniPath>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge)]
#[serde(deny_unknown_fields)]
pub struct CacheLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub key: Option<CacheKeyLayer>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub enabled: Option<Replace<bool>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub output: Option<TaskOutputLayer>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge)]
#[serde(deny_unknown_fields)]
pub struct ProjectEnvLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub vars: Option<DictConfig<Replace<String>>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge)]
#[serde(deny_unknown_fields)]
pub struct TaskLongFormLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace)]
    pub base: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub extends: Option<SingleOrMany<String>>,

    #[serde(
        default,
        alias = "command",
        skip_serializing_if = "Option::is_none"
    )]
    #[merge(strategy = merge::option::recurse)]
    pub exec: Option<Replace<CommandConfig>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub retry_exec: Option<Replace<CommandConfig>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub args: Option<DictConfig<DynValue>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub dependencies: Option<ListConfig<TaskDependencyConfiguration>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub with: Option<ListConfig<TaskDependencyConfiguration>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub description: Option<Replace<String>>,

    #[serde(default, alias = "if", skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub enabled: Option<TeraExprBoolean>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub interactive: Option<Replace<bool>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub persistent: Option<Replace<bool>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub env: Option<TaskEnvConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub redact: Option<RedactConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub cache: Option<CacheLayer>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub output_logs: Option<OutputLogsConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub meta: Option<MetaConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub retry: Option<RetryConfiguration>,
}

/// A task as written: a bare command or a long-form block. Short form is kept
/// as written; promoting it to long form is left to finalization.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum TaskLayer {
    ShortForm(CommandConfig),
    LongForm(Box<TaskLongFormLayer>),
}

impl<'a> Deserialize<'a> for TaskLayer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        use omni_serde_validators::tera_expr::validate_str;
        serde_untagged::UntaggedEnumVisitor::new()
            .string(|s| {
                validate_str(&s).map_err(serde::de::Error::custom)?;
                Ok(TaskLayer::ShortForm(CommandConfig::Shell(s.to_string())))
            })
            .seq(|seq| {
                let items: Vec<String> = seq.deserialize()?;
                for item in &items {
                    validate_str(item).map_err(serde::de::Error::custom)?;
                }
                Ok(TaskLayer::ShortForm(CommandConfig::Argv(items)))
            })
            .map(|long_form| long_form.deserialize().map(TaskLayer::LongForm))
            .deserialize(deserializer)
    }
}

impl Merge for TaskLayer {
    fn merge(&mut self, other: Self) {
        match (self, other) {
            (TaskLayer::LongForm(a), TaskLayer::LongForm(b)) => a.merge(*b),
            (this, other) => *this = other,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge)]
#[serde(deny_unknown_fields)]
pub struct ProjectLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace)]
    pub base: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub extends: Option<SingleOrMany<OmniPath>>,

    /// Required only once layers are finalized, not when one is parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub description: Option<Replace<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub dependencies: Option<ListConfig<Replace<String>>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub env: Option<ProjectEnvLayer>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub cache: Option<CacheLayer>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub output_logs: Option<OutputLogsConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub meta: Option<MetaConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub redact: Option<RedactConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub tasks: Option<DictConfig<TaskLayer>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectConfiguration;

    fn layer(json: &str) -> ProjectLayer {
        serde_json::from_str(json).unwrap()
    }

    fn eager(json: &str) -> ProjectConfiguration {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn empty_layer_parses_with_every_field_absent() {
        assert_eq!(layer("{}"), ProjectLayer::default());
        assert!(serde_json::from_str::<ProjectConfiguration>("{}").is_err());
    }

    #[test]
    fn null_is_absent() {
        assert_eq!(layer(r#"{"name":null,"cache":null}"#), layer("{}"));
    }

    #[test]
    fn explicit_defaults_are_present() {
        let l = layer(
            r#"{"base":false,"dependencies":[],"cache":{"enabled":true,
            "key":{"defaults":false,"env":[],"files":[]},
            "output":{"files":[],"logs":true}},
            "env":{"vars":{}},"meta":{},"tasks":{}}"#,
        );
        assert_eq!(l.base, Some(false));
        assert_eq!(l.dependencies, Some(ListConfig::value(vec![])));
        let cache = l.cache.unwrap();
        assert_eq!(cache.enabled, Some(Replace::new(true)));
        let key = cache.key.unwrap();
        assert_eq!(key.defaults, Some(false));
        assert_eq!(key.env, Some(ListConfig::value(vec![])));
        assert!(key.files.is_some());
        let out = cache.output.unwrap();
        assert!(out.files.is_some());
        assert_eq!(out.logs, Some(true));
        assert!(l.env.unwrap().vars.is_some());
        assert!(l.meta.is_some());
        assert!(l.tasks.is_some());
    }

    #[test]
    fn omitted_nested_fields_stay_absent() {
        let l = layer(r#"{"cache":{"key":{"env":["A"]}}}"#);
        let cache = l.cache.unwrap();
        assert!(cache.enabled.is_none() && cache.output.is_none());
        let key = cache.key.unwrap();
        assert!(key.defaults.is_none() && key.files.is_none());
    }

    #[test]
    fn glob_sides_keep_their_presence() {
        let l = layer(r#"{"cache":{"key":{"files":{"exclude":["x"]}}}}"#);
        let files = l.cache.unwrap().key.unwrap().files.unwrap();
        assert!(matches!(
            files,
            MergeGlobConfig::IncludeAndExclude {
                include: None,
                exclude: Some(_)
            }
        ));
    }

    #[test]
    fn list_and_dict_operators_are_kept() {
        let l = layer(
            r#"{"dependencies":{"prepend":["a"]},
            "env":{"vars":{"replace":{"A":"1"}}},
            "cache":{"key":{"env":{"replace":["X"]}}}}"#,
        );
        assert!(l.dependencies.unwrap().is_prepend());
        assert!(l.env.unwrap().vars.unwrap().is_replace());
        assert!(l.cache.unwrap().key.unwrap().env.unwrap().is_replace());
    }

    #[test]
    fn mixed_operator_envelope_is_rejected() {
        assert!(
            serde_json::from_str::<ProjectLayer>(
                r#"{"dependencies":{"append":[],"replace":["x"]}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(
            serde_json::from_str::<ProjectLayer>(r#"{"bogus":1}"#).is_err()
        );
        assert!(
            serde_json::from_str::<ProjectLayer>(r#"{"cache":{"bogus":1}}"#)
                .is_err()
        );
    }

    #[test]
    fn task_forms_and_aliases() {
        let l = layer(
            r#"{"tasks":{"a":"echo hi","b":["x","y"],
            "c":{"command":"c","if":"true","cache":{"enabled":false}}}}"#,
        );
        let tasks = l.tasks.unwrap();
        let tasks = tasks.as_map();
        assert_eq!(
            tasks["a"],
            TaskLayer::ShortForm(CommandConfig::Shell("echo hi".into()))
        );
        assert!(matches!(
            tasks["b"],
            TaskLayer::ShortForm(CommandConfig::Argv(_))
        ));
        let TaskLayer::LongForm(c) = &tasks["c"] else {
            panic!("long form")
        };
        assert!(c.exec.is_some() && c.enabled.is_some());
        assert!(c.interactive.is_none() && c.retry.is_none());
    }

    #[test]
    fn absent_field_does_not_override_on_merge() {
        let mut base = layer(
            r#"{"name":"a","cache":{"enabled":false,"key":{"defaults":false}}}"#,
        );
        base.merge(layer(r#"{"cache":{"key":{"env":["E"]}}}"#));
        let cache = base.cache.unwrap();
        assert_eq!(cache.enabled, Some(Replace::new(false)));
        assert_eq!(cache.key.unwrap().defaults, Some(false));
        assert_eq!(base.name.as_deref(), Some("a"));
    }

    #[test]
    fn explicit_default_overrides_on_merge() {
        let mut base = layer(r#"{"cache":{"key":{"defaults":false}}}"#);
        base.merge(layer(r#"{"cache":{"key":{"defaults":true}}}"#));
        assert_eq!(base.cache.unwrap().key.unwrap().defaults, Some(true));
    }

    #[test]
    fn base_is_replaced_not_inherited() {
        let mut l = layer(r#"{"base":true}"#);
        l.merge(layer(r#"{"name":"c"}"#));
        assert_eq!(l.base, None);
    }

    /// Replaying layers equals merging the eager types, for the fields whose
    /// eager defaults are the identity of the merge.
    #[test]
    fn replay_matches_eager_merge() {
        let docs = [
            r#"{"name":"a","extends":"x.yaml","dependencies":["d1"],
            "env":{"vars":{"A":"1","B":"2"}},
            "cache":{"enabled":true,"key":{"env":["E1"],"files":["f1"]},
                     "output":{"files":["o1"],"logs":false}},
            "tasks":{"t":{"exec":"a","dependencies":["^t"]},"s":"echo"}}"#,
            r#"{"name":"b","dependencies":{"prepend":["d0"]},
            "env":{"vars":{"merge":{"B":"3"}}},
            "cache":{"key":{"env":{"replace":["E2"]},"files":{"exclude":["f2"]}}},
            "tasks":{"t":{"exec":"b","cache":{"enabled":false}}}}"#,
            r#"{"name":"c","dependencies":{"append":["d2"]},
            "env":{"vars":{"replace":{"C":"4"}}},
            "tasks":{"s":{"exec":"z"},"u":"u"}}"#,
        ];
        let mut e = eager(docs[0]);
        let mut l = layer(docs[0]);
        for d in &docs[1..] {
            e.merge(eager(d));
            l.merge(layer(d));
        }
        assert_eq!(l.name.as_ref(), Some(&e.name));
        assert_eq!(l.extends, e.extends);
        assert_eq!(l.dependencies.unwrap(), e.dependencies);
        assert_eq!(l.env.unwrap().vars.unwrap(), e.env.vars);
        let (lc, ec) = (l.cache.unwrap(), e.cache);
        assert_eq!(lc.enabled, ec.enabled);
        let lk = lc.key.unwrap();
        // The eager type folds the operator into its default; the layer keeps
        // it, so compare the resulting values.
        assert_eq!(lk.env.unwrap().into_vec(), ec.key.env.into_vec());
        assert_eq!(lk.files.unwrap().normalize(), ec.key.files.normalize());
        let lo = lc.output.unwrap();
        assert_eq!(lo.files.unwrap().normalize(), ec.output.files.normalize());
        // Known divergence: the eager `logs` is a plain bool, so a later layer
        // that omits it resets it to `true`. The layer keeps the earlier value.
        assert_eq!(lo.logs, Some(false));
        assert!(ec.output.logs);
        let lt = l.tasks.unwrap();
        let mut keys: Vec<_> = lt.keys().cloned().collect();
        let mut ekeys: Vec<_> = e.tasks.keys().cloned().collect();
        keys.sort();
        ekeys.sort();
        assert_eq!(keys, ekeys);
        let TaskLayer::LongForm(t) = &lt.as_map()["t"] else {
            panic!()
        };
        let crate::TaskConfiguration::LongForm(et) = &e.tasks.as_map()["t"]
        else {
            panic!()
        };
        assert_eq!(t.exec, et.exec);
        assert_eq!(
            t.dependencies.clone().unwrap().into_vec(),
            et.dependencies.clone().into_vec()
        );
        assert_eq!(t.cache.as_ref().unwrap().enabled, et.cache.enabled);
    }
}
