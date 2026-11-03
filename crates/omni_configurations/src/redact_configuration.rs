use omni_glob::name::{Case, NameGlobOptions, NameGlobSet, Separator};
use omni_redact::{Redactor, RedactorBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a policy treats names it does not list, plus the names it does.
///
/// Written either as a bare list (shorthand for `only`) or as an object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RedactPolicyConfiguration {
    /// Shorthand: names added to `only`. Never changes `all`.
    Only(Vec<String>),
    Detailed(RedactPolicyDetail),
}

#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct RedactPolicyDetail {
    /// When true, every value whose name is not listed is redacted. Inherited
    /// from the nearest level that sets it; the category default applies at
    /// the root (`true` for env, `false` for args and inputs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,

    /// Names or globs that are always redacted. Unioned across levels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,

    /// Names or globs that stay visible under `all: true`, unless also matched
    /// by `only` at any level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub except: Vec<String>,
}

impl RedactPolicyConfiguration {
    pub fn detail(&self) -> RedactPolicyDetail {
        match self {
            Self::Only(only) => RedactPolicyDetail {
                all: None,
                only: only.clone(),
                except: vec![],
            },
            Self::Detailed(d) => d.clone(),
        }
    }

    fn patterns(&self) -> Vec<&String> {
        match self {
            Self::Only(only) => only.iter().collect(),
            Self::Detailed(d) => d.only.iter().chain(&d.except).collect(),
        }
    }
}

/// Which values are redacted from logs, diagnostics and dry-run output.
///
/// Display only: execution and cache keys are unaffected.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct RedactConfiguration {
    /// Redaction mode. Workspace level only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RedactMode>,

    /// A task's free-form `args`, including `-a/--arg` values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<RedactPolicyConfiguration>,

    /// Schema-declared inputs of tools and generators. Workspace level only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<RedactPolicyConfiguration>,

    /// Environment values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<RedactPolicyConfiguration>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum RedactMode {
    /// Apply the resolved policies.
    #[default]
    Declared,
    /// Treat every category as `all: true` with no `except`.
    All,
    /// Disable redaction.
    None,
}

fn merge_policy(
    base: &mut Option<RedactPolicyConfiguration>,
    other: Option<RedactPolicyConfiguration>,
) {
    let Some(other) = other else { return };
    let Some(current) = base.take() else {
        *base = Some(other);
        return;
    };
    let mut a = current.detail();
    let b = other.detail();
    a.all = b.all.or(a.all);
    union(&mut a.only, b.only);
    union(&mut a.except, b.except);
    *base = Some(RedactPolicyConfiguration::Detailed(a));
}

fn union(into: &mut Vec<String>, from: Vec<String>) {
    for v in from {
        if !into.contains(&v) {
            into.push(v);
        }
    }
}

impl merge::Merge for RedactConfiguration {
    fn merge(&mut self, other: Self) {
        if other.mode.is_some() {
            self.mode = other.mode;
        }
        merge_policy(&mut self.args, other.args);
        merge_policy(&mut self.inputs, other.inputs);
        merge_policy(&mut self.env, other.env);
    }
}

impl RedactConfiguration {
    /// Validates patterns. `workspace` allows `mode` and `inputs`.
    pub fn check(&self, workspace: bool) -> Result<(), String> {
        if !workspace {
            if self.mode.is_some() {
                return Err(
                    "`redact.mode` is only valid in the workspace configuration"
                        .into(),
                );
            }
            if self.inputs.is_some() {
                return Err(
                    "`redact.inputs` is only valid in the workspace configuration"
                        .into(),
                );
            }
        }
        for (key, policy, opts) in [
            ("args", &self.args, RedactCategory::Args.glob_options()),
            (
                "inputs",
                &self.inputs,
                RedactCategory::Inputs.glob_options(),
            ),
            ("env", &self.env, RedactCategory::Env.glob_options()),
        ] {
            if let Some(p) = policy {
                NameGlobSet::new(p.patterns(), &opts)
                    .map_err(|e| format!("`redact.{key}`: {e}"))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactCategory {
    Args,
    Inputs,
    Env,
}

impl RedactCategory {
    pub fn glob_options(self) -> NameGlobOptions {
        match self {
            Self::Args | Self::Inputs => {
                NameGlobOptions::default().separator(Separator::Char('.'))
            }
            Self::Env => NameGlobOptions::default().case(if cfg!(windows) {
                Case::Insensitive
            } else {
                Case::Sensitive
            }),
        }
    }

    fn default_all(self) -> bool {
        matches!(self, Self::Env)
    }
}

/// Command-line redaction overrides (`--redact`, `--redact-arg`,
/// `--redact-input`, `--redact-env`). They add names and may change the mode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RedactOverrides {
    pub mode: Option<RedactMode>,
    pub args: Vec<String>,
    pub inputs: Vec<String>,
    pub env: Vec<String>,
}

impl RedactOverrides {
    pub fn apply(&self, policy: RedactPolicy) -> RedactPolicy {
        policy.with_overrides(self.mode, &self.args, &self.inputs, &self.env)
    }
}

/// A resolved policy for one category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactCategoryPolicy {
    pub all: bool,
    pub only: Vec<String>,
    pub except: Vec<String>,
}

/// The resolved policy for a task (or a tool/generator run), plus the mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactPolicy {
    pub mode: RedactMode,
    pub args: RedactCategoryPolicy,
    pub inputs: RedactCategoryPolicy,
    pub env: RedactCategoryPolicy,
}

impl Default for RedactPolicy {
    fn default() -> Self {
        Self::resolve(&[])
    }
}

impl RedactPolicy {
    /// Resolve `levels`, ordered from the outermost (workspace) to the
    /// innermost (task). `only` and `except` are unioned across levels and
    /// `all` comes from the nearest level that sets it.
    pub fn resolve(levels: &[&RedactConfiguration]) -> Self {
        let mut merged = RedactConfiguration::default();
        for level in levels {
            merge::Merge::merge(&mut merged, (*level).clone());
        }
        let cat = |c: RedactCategory, p: &Option<RedactPolicyConfiguration>| {
            let d = p.as_ref().map(|p| p.detail()).unwrap_or_default();
            RedactCategoryPolicy {
                all: d.all.unwrap_or_else(|| c.default_all()),
                only: d.only,
                except: d.except,
            }
        };
        Self {
            mode: merged.mode.unwrap_or_default(),
            args: cat(RedactCategory::Args, &merged.args),
            inputs: cat(RedactCategory::Inputs, &merged.inputs),
            env: cat(RedactCategory::Env, &merged.env),
        }
    }

    pub fn category(&self, c: RedactCategory) -> &RedactCategoryPolicy {
        match c {
            RedactCategory::Args => &self.args,
            RedactCategory::Inputs => &self.inputs,
            RedactCategory::Env => &self.env,
        }
    }

    /// Apply command-line overrides: extra names are added to `only` and a
    /// mode, when given, replaces the configured one. The CLI can tighten what
    /// config declares, never loosen it, except through `mode`.
    pub fn with_overrides(
        mut self,
        mode: Option<RedactMode>,
        args: &[String],
        inputs: &[String],
        env: &[String],
    ) -> Self {
        if let Some(mode) = mode {
            self.mode = mode;
        }
        union(&mut self.args.only, args.to_vec());
        union(&mut self.inputs.only, inputs.to_vec());
        union(&mut self.env.only, env.to_vec());
        self
    }

    /// `only` and `except` patterns of `category` that match none of `names`.
    /// Used for debug-level typo hints; never an error.
    pub fn unmatched_patterns<'n>(
        &self,
        category: RedactCategory,
        names: impl IntoIterator<Item = &'n str> + Clone,
    ) -> Vec<String> {
        let policy = self.category(category);
        let opts = category.glob_options();
        let mut out = Vec::new();
        for list in [&policy.only, &policy.except] {
            if let Ok(set) = NameGlobSet::new(list, &opts) {
                out.extend(
                    set.unmatched(names.clone()).into_iter().map(String::from),
                );
            }
        }
        out
    }

    /// Builds a [`Redactor`] for `category` from `(name, value)` entries.
    pub fn redactor<'a>(
        &self,
        category: RedactCategory,
        entries: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Redactor {
        self.redactor_builder(category, entries).build()
    }

    pub fn redactor_builder<'a>(
        &self,
        category: RedactCategory,
        entries: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> RedactorBuilder {
        let builder = Redactor::builder();
        let policy = match self.mode {
            RedactMode::None => return builder,
            RedactMode::Declared => self.category(category).clone(),
            RedactMode::All => RedactCategoryPolicy {
                all: true,
                only: vec![],
                except: vec![],
            },
        };
        policy.collect(category, entries, builder)
    }
}

impl RedactCategoryPolicy {
    fn collect<'a>(
        &self,
        category: RedactCategory,
        entries: impl IntoIterator<Item = (&'a str, &'a str)>,
        mut builder: RedactorBuilder,
    ) -> RedactorBuilder {
        let opts = category.glob_options();
        // Patterns were validated at load; an invalid one redacts nothing
        // rather than panicking, and fails closed by redacting everything.
        let only = NameGlobSet::new(&self.only, &opts);
        let except = NameGlobSet::new(&self.except, &opts);
        let (Ok(only), Ok(except)) = (only, except) else {
            return entries.into_iter().fold(builder, |b, (_, v)| b.value(v));
        };
        for (name, value) in entries {
            if only.is_match(name) {
                builder = builder.value(value);
            } else if self.all && !except.is_match(name) {
                builder = builder.bulk_value(value);
            }
        }
        builder
    }
}

/// Flattens a JSON value into `(dotted path, leaf string)` pairs, descending
/// into objects and arrays. Used so a dotted pattern can address a nested arg.
pub fn flatten_json_leaves(
    prefix: &str,
    value: &serde_json::Value,
    out: &mut Vec<(String, String)>,
) {
    use serde_json::Value;
    match value {
        Value::Null => {}
        Value::String(s) => out.push((prefix.to_string(), s.clone())),
        Value::Bool(_) | Value::Number(_) => {
            out.push((prefix.to_string(), value.to_string()))
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                flatten_json_leaves(&format!("{prefix}.{i}"), v, out);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_json_leaves(&path, v, out);
            }
        }
    }
}

impl RedactPolicy {
    /// Builds a [`Redactor`] over a JSON map of args or inputs. A pattern that
    /// matches a key (or any ancestor of a leaf) redacts every leaf below it.
    pub fn redactor_for_json<'a>(
        &self,
        category: RedactCategory,
        entries: impl IntoIterator<Item = (&'a str, &'a serde_json::Value)>,
    ) -> Redactor {
        let mut leaves = Vec::new();
        for (k, v) in entries {
            flatten_json_leaves(k, v, &mut leaves);
        }
        let expanded = leaves.iter().map(|(p, v)| (p.as_str(), v.as_str()));
        self.redactor_by_ancestors(category, expanded)
    }

    fn redactor_by_ancestors<'a>(
        &self,
        category: RedactCategory,
        leaves: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Redactor {
        let opts = category.glob_options();
        let builder = Redactor::builder();
        let policy = match self.mode {
            RedactMode::None => return builder.build(),
            RedactMode::Declared => self.category(category).clone(),
            RedactMode::All => RedactCategoryPolicy {
                all: true,
                only: vec![],
                except: vec![],
            },
        };
        let (Ok(only), Ok(except)) = (
            NameGlobSet::new(&policy.only, &opts),
            NameGlobSet::new(&policy.except, &opts),
        ) else {
            return leaves
                .into_iter()
                .fold(builder, |b, (_, v)| b.value(v))
                .build();
        };
        let any_ancestor = |set: &NameGlobSet, path: &str| {
            let mut p = path;
            loop {
                if set.is_match(p) {
                    return true;
                }
                match p.rfind('.') {
                    Some(i) => p = &p[..i],
                    None => return false,
                }
            }
        };
        let mut b = builder;
        for (path, value) in leaves {
            if any_ancestor(&only, path) {
                b = b.value(value);
            } else if policy.all && !any_ancestor(&except, path) {
                b = b.bulk_value(value);
            }
        }
        b.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(json: serde_json::Value) -> RedactConfiguration {
        serde_json::from_value(json).unwrap()
    }

    fn red(
        p: &RedactPolicy,
        c: RedactCategory,
        e: &[(&str, &str)],
        text: &str,
    ) -> String {
        p.redactor(c, e.iter().copied()).redact(text).into_owned()
    }

    #[test]
    fn shorthand_adds_to_only_without_changing_all() {
        let c = cfg(json!({"env": ["AWS_*"]}));
        let p = RedactPolicy::resolve(&[&c]);
        assert!(p.env.all);
        assert_eq!(p.env.only, vec!["AWS_*"]);
        let c = cfg(json!({"args": ["token"]}));
        let p = RedactPolicy::resolve(&[&c]);
        assert!(!p.args.all);
    }

    #[test]
    fn defaults() {
        let p = RedactPolicy::default();
        assert!(p.env.all && !p.args.all && !p.inputs.all);
        assert_eq!(p.mode, RedactMode::Declared);
    }

    #[test]
    fn only_is_unioned_and_all_comes_from_nearest() {
        let ws = cfg(json!({"env": {"all": true, "only": ["A"]}}));
        let task = cfg(json!({"env": {"all": false, "only": ["B"]}}));
        let p = RedactPolicy::resolve(&[&ws, &task]);
        assert!(!p.env.all);
        assert_eq!(p.env.only, vec!["A", "B"]);
    }

    #[test]
    fn only_beats_except_across_levels() {
        let ws = cfg(json!({"env": {"only": ["SECRET"]}}));
        let task =
            cfg(json!({"env": {"all": true, "except": ["SECRET", "CI"]}}));
        let p = RedactPolicy::resolve(&[&ws, &task]);
        let e = [("SECRET", "s3cr3t-value"), ("CI", "ci-value-xx")];
        assert_eq!(
            red(&p, RedactCategory::Env, &e, "s3cr3t-value ci-value-xx"),
            "<redacted> ci-value-xx"
        );
    }

    #[test]
    fn short_values_under_all_are_kept_but_listed_names_are_hidden() {
        let p = RedactPolicy::resolve(&[&cfg(json!({"env": {"all": true}}))]);
        let e = [("A", "1"), ("B", "true"), ("C", "abcdefgh")];
        assert_eq!(
            red(&p, RedactCategory::Env, &e, "1 ab abcdefgh"),
            "1 ab <redacted>"
        );
        let p = RedactPolicy::resolve(&[&cfg(
            json!({"env": {"all": true, "only": ["A"]}}),
        )]);
        assert_eq!(
            red(&p, RedactCategory::Env, &e, "1 ab abcdefgh"),
            "<redacted> ab <redacted>"
        );
    }

    #[test]
    fn dotted_args_and_ancestors() {
        let p = RedactPolicy::resolve(&[&cfg(
            json!({"args": ["db.password", "token"]}),
        )]);
        let args = serde_json::json!({
            "db": { "password": "hunter22", "host": "localhost" },
            "token": { "a": "tok-one", "b": "tok-two" }
        });
        let r = p.redactor_for_json(
            RedactCategory::Args,
            args.as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.as_str(), v)),
        );
        assert_eq!(
            r.redact("hunter22 localhost tok-one tok-two"),
            "<redacted> localhost <redacted> <redacted>"
        );
    }

    #[test]
    fn mode_all_and_none() {
        let p = RedactPolicy::resolve(&[&cfg(json!({"env": ["A"]}))]);
        let e = [("A", "aaaaaa"), ("B", "bbbbbb")];
        let all =
            p.clone()
                .with_overrides(Some(RedactMode::All), &[], &[], &[]);
        assert_eq!(
            red(&all, RedactCategory::Args, &e, "aaaaaa bbbbbb"),
            "<redacted> <redacted>"
        );
        let none = p.with_overrides(Some(RedactMode::None), &[], &[], &[]);
        assert_eq!(red(&none, RedactCategory::Env, &e, "aaaaaa"), "aaaaaa");
    }

    #[test]
    fn cli_overrides_add_names() {
        let p = RedactPolicy::default().with_overrides(
            None,
            &["token".into()],
            &[],
            &["X".into()],
        );
        assert_eq!(p.args.only, vec!["token"]);
        assert_eq!(p.env.only, vec!["X"]);
    }

    #[test]
    fn scope_validation() {
        assert!(cfg(json!({"inputs": ["a"]})).check(true).is_ok());
        assert!(cfg(json!({"inputs": ["a"]})).check(false).is_err());
        assert!(cfg(json!({"mode": "none"})).check(false).is_err());
        assert!(cfg(json!({"env": ["a\\"]})).check(true).is_err());
        assert!(
            serde_json::from_value::<RedactConfiguration>(json!({"bogus": 1}))
                .is_err()
        );
    }

    #[test]
    fn same_level_merge_unions() {
        let mut a = cfg(json!({"args": ["x"]}));
        merge::Merge::merge(
            &mut a,
            cfg(json!({"args": {"all": true, "only": ["y"]}})),
        );
        let p = RedactPolicy::resolve(&[&a]);
        assert!(p.args.all);
        assert_eq!(p.args.only, vec!["x", "y"]);
    }
}
