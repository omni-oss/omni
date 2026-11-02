use std::{cell::RefCell, fmt, path::PathBuf};

use maps::UnorderedMap;
use omni_configurations::MetaConfiguration;
use omni_core::TaskExecutionNode;
use omni_expressions::ErrorClass;

const MAX_SNAPSHOT_KEYS: usize = 20;
const MAX_SNAPSHOT_VALUE_CHARS: usize = 256;
const SIMILARITY_THRESHOLD: f64 = 0.8;

const CEL_FUNCTIONS: &[&str] = &[
    "startsWith",
    "endsWith",
    "contains",
    "matches",
    "size",
    "has",
    "exists",
    "exists_one",
    "all",
    "map",
    "filter",
    "int",
    "uint",
    "double",
    "string",
    "bytes",
    "bool",
    "min",
    "max",
];

/// What to do with tasks whose meta filter fails to evaluate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetaOnError {
    /// Abort planning with an aggregated error.
    #[default]
    Fail,
    /// Exclude the failing tasks and report them as a warning.
    Exclude,
}

/// Bounds on the memory used to report meta filter failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetaFilterErrorLimits {
    pub samples_per_cause: usize,
    pub max_causes: usize,
}

impl MetaFilterErrorLimits {
    pub const DEFAULT_SAMPLES_PER_CAUSE: usize = 3;
    pub const MAX_CAUSES: usize = 10;

    pub fn with_samples(samples_per_cause: usize) -> Self {
        Self {
            samples_per_cause,
            max_causes: Self::MAX_CAUSES,
        }
    }
}

impl Default for MetaFilterErrorLimits {
    fn default() -> Self {
        Self::with_samples(Self::DEFAULT_SAMPLES_PER_CAUSE)
    }
}

/// How meta filter failures are reported by the plan provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetaErrorOptions {
    pub on_error: MetaOnError,
    pub limits: MetaFilterErrorLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaFilterSample {
    pub project: String,
    pub task: String,
    pub project_dir: PathBuf,
    /// Truncated `(key, json value)` pairs.
    pub meta: Vec<(String, String)>,
}

impl MetaFilterSample {
    fn render_meta(&self) -> String {
        if self.meta.is_empty() {
            return "{}".to_string();
        }
        let items = self
            .meta
            .iter()
            .map(|(k, v)| format!("\"{k}\": {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{ {items} }}")
    }

    fn name(&self) -> String {
        format!("{}#{}", self.project, self.task)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaFilterGroup {
    pub cause: String,
    pub count: usize,
    pub samples: Vec<MetaFilterSample>,
    pub omitted: usize,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaFilterError {
    pub filter: String,
    pub total: usize,
    pub groups: Vec<MetaFilterGroup>,
    /// Failures whose cause was not retained because of the cause limit.
    pub other: usize,
}

impl MetaFilterError {
    /// Compact multi-line rendering used when failing tasks are excluded.
    pub fn render_warning(&self) -> String {
        let mut out = format!(
            "meta filter `{}` failed for {} task(s); they were excluded (see --meta-on-error)",
            self.filter, self.total
        );
        for g in &self.groups {
            for s in &g.samples {
                out.push_str(&format!(
                    "\n  {}: {}   meta: {}",
                    s.name(),
                    g.cause,
                    s.render_meta()
                ));
            }
            if g.omitted > 0 {
                out.push_str(&format!(
                    "\n  {}: ... and {} more",
                    g.cause, g.omitted
                ));
            }
        }
        if self.other > 0 {
            out.push_str(&format!(
                "\n  ... and {} more with other causes",
                self.other
            ));
        }
        out
    }
}

impl fmt::Display for MetaFilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.total == 1
            && let Some(g) = self.groups.first()
            && let Some(s) = g.samples.first()
        {
            writeln!(f, "meta filter failed to evaluate")?;
            writeln!(f)?;
            writeln!(f, "  filter:   {}", self.filter)?;
            writeln!(f, "  cause:    {}", g.cause)?;
            writeln!(f)?;
            writeln!(f, "  task:     {}", s.name())?;
            writeln!(
                f,
                "  project:  {}  ({})",
                s.project,
                s.project_dir.display()
            )?;
            write!(f, "  meta:     {}", s.render_meta())?;
            if let Some(h) = &g.hint {
                write!(f, "\n\n  hint: {h}")?;
            }
            return write!(
                f,
                "\n        To skip failing tasks instead, pass --meta-on-error exclude."
            );
        }

        writeln!(
            f,
            "meta filter failed to evaluate for {} task(s)",
            self.total
        )?;
        writeln!(f)?;
        writeln!(f, "  filter:  {}", self.filter)?;
        for g in &self.groups {
            writeln!(f, "  cause:   {}   [{} tasks]", g.cause, g.count)?;
            if !g.samples.is_empty() {
                writeln!(f)?;
            }
            for s in &g.samples {
                writeln!(
                    f,
                    "    {}  {}  meta: {}",
                    s.name(),
                    s.project_dir.display(),
                    s.render_meta()
                )?;
            }
            if g.omitted > 0 {
                writeln!(f, "    ... and {} more", g.omitted)?;
            }
            if let Some(h) = &g.hint {
                writeln!(f)?;
                writeln!(f, "  hint: {h}")?;
            }
            writeln!(f)?;
        }
        if self.other > 0 {
            writeln!(f, "  ... and {} more with other causes\n", self.other)?;
        }
        write!(
            f,
            "  To skip failing tasks instead, pass --meta-on-error exclude."
        )
    }
}

impl std::error::Error for MetaFilterError {}

struct GroupState {
    cause: String,
    class: ErrorClass,
    first_keys: Vec<String>,
    count: usize,
    samples: Vec<MetaFilterSample>,
}

#[derive(Default)]
struct State {
    total: usize,
    other: usize,
    groups: Vec<GroupState>,
    index: UnorderedMap<String, usize>,
}

/// Aggregates meta filter failures in memory bounded by the configured limits.
pub struct MetaFilterCollector {
    filter: String,
    limits: MetaFilterErrorLimits,
    state: RefCell<State>,
}

impl MetaFilterCollector {
    pub fn new(filter: impl Into<String>, limits: MetaFilterErrorLimits) -> Self {
        Self {
            filter: filter.into(),
            limits,
            state: RefCell::new(State::default()),
        }
    }

    pub fn record(
        &self,
        node: &TaskExecutionNode,
        error: &omni_expressions::Error,
        meta: Option<&MetaConfiguration>,
    ) {
        let cause = error.to_string();
        let mut st = self.state.borrow_mut();
        st.total += 1;

        let idx = match st.index.get(&cause) {
            Some(i) => *i,
            None => {
                if st.groups.len() >= self.limits.max_causes {
                    st.other += 1;
                    return;
                }
                let i = st.groups.len();
                st.groups.push(GroupState {
                    cause: cause.clone(),
                    class: error.class(),
                    first_keys: meta_keys(meta),
                    count: 0,
                    samples: vec![],
                });
                st.index.insert(cause, i);
                i
            }
        };

        let group = &mut st.groups[idx];
        group.count += 1;
        if group.samples.len() < self.limits.samples_per_cause {
            group.samples.push(MetaFilterSample {
                project: node.project_name().to_string(),
                task: node.task_name().to_string(),
                project_dir: node.project_dir().to_path_buf(),
                meta: snapshot(meta),
            });
        }
    }

    pub fn is_empty(&self) -> bool {
        self.state.borrow().total == 0
    }

    /// Takes the aggregate, leaving the collector empty.
    pub fn take(&self) -> Option<MetaFilterError> {
        let st = std::mem::take(&mut *self.state.borrow_mut());
        if st.total == 0 {
            return None;
        }
        let groups = st
            .groups
            .into_iter()
            .map(|g| MetaFilterGroup {
                hint: hint(&g.class, &g.first_keys),
                omitted: g.count - g.samples.len(),
                cause: g.cause,
                count: g.count,
                samples: g.samples,
            })
            .collect();
        Some(MetaFilterError {
            filter: self.filter.clone(),
            total: st.total,
            groups,
            other: st.other,
        })
    }
}

fn meta_keys(meta: Option<&MetaConfiguration>) -> Vec<String> {
    meta.map(|m| {
        m.0.iter()
            .take(MAX_SNAPSHOT_KEYS)
            .map(|(k, _)| k.to_string())
            .collect()
    })
    .unwrap_or_default()
}

fn snapshot(meta: Option<&MetaConfiguration>) -> Vec<(String, String)> {
    let Some(meta) = meta else {
        return vec![];
    };
    let total = meta.0.iter().count();
    let mut out = meta
        .0
        .iter()
        .take(MAX_SNAPSHOT_KEYS)
        .map(|(k, v)| {
            let mut s = v.clone().into_json().to_string();
            if s.chars().count() > MAX_SNAPSHOT_VALUE_CHARS {
                s = s.chars().take(MAX_SNAPSHOT_VALUE_CHARS).collect::<String>()
                    + "…";
            }
            (k.to_string(), s)
        })
        .collect::<Vec<_>>();
    if total > MAX_SNAPSHOT_KEYS {
        out.push((
            "…".to_string(),
            format!("{} more keys", total - MAX_SNAPSHOT_KEYS),
        ));
    }
    out
}

fn closest<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    candidates
        .into_iter()
        .map(|c| (c, strsim::jaro_winkler(name, c)))
        .filter(|(_, score)| *score >= SIMILARITY_THRESHOLD)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(c, _)| c)
}

pub(crate) fn hint(class: &ErrorClass, keys: &[String]) -> Option<String> {
    match class {
        ErrorClass::MissingReference { name, is_call: false } => {
            let mut h = format!("`{name}` is not defined for this task.");
            if let Some(k) = closest(name, keys.iter().map(|k| k.as_str())) {
                h.push_str(&format!(" Did you mean `{k}`?"));
            }
            h.push_str(&format!(" For optional keys use has(meta.{name})."));
            Some(h)
        }
        ErrorClass::MissingReference { name, is_call: true } => {
            let mut h = format!("`{name}` is not a known function.");
            if let Some(f) = closest(name, CEL_FUNCTIONS.iter().copied()) {
                h.push_str(&format!(" Did you mean `{f}`?"));
            }
            Some(h)
        }
        ErrorClass::MissingKey { key } => Some(format!(
            "`{key}` is missing in a nested map. Use has(meta.a.{key}) to test for it."
        )),
        ErrorClass::TypeMismatch { detail } => {
            Some(format!("Operand types differ: {detail}"))
        }
        ErrorClass::NonBool { got } => Some(format!(
            "Filter must be a bool, got `{got}`. Compare explicitly, e.g. `owner != \"\"` or `size(tags) > 0`."
        )),
        ErrorClass::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_suggests_close_key() {
        let h = hint(
            &ErrorClass::MissingReference {
                name: "knd".into(),
                is_call: false,
            },
            &["kind".into(), "owner".into()],
        )
        .unwrap();
        assert!(h.contains("Did you mean `kind`"), "{h}");
        assert!(h.contains("has(meta.knd)"));
    }

    #[test]
    fn hint_without_close_key_is_generic() {
        let h = hint(
            &ErrorClass::MissingReference {
                name: "zzz".into(),
                is_call: false,
            },
            &["kind".into()],
        )
        .unwrap();
        assert!(!h.contains("Did you mean"));
        assert!(h.contains("has(meta.zzz)"));
    }

    #[test]
    fn hint_suggests_function() {
        let h = hint(
            &ErrorClass::MissingReference {
                name: "startWith".into(),
                is_call: true,
            },
            &[],
        )
        .unwrap();
        assert!(h.contains("`startsWith`"), "{h}");
    }

    #[test]
    fn hint_other_is_none() {
        assert!(hint(&ErrorClass::Other, &[]).is_none());
        assert!(
            hint(&ErrorClass::NonBool { got: "string".into() }, &[])
                .unwrap()
                .contains("got `string`")
        );
        assert!(
            hint(&ErrorClass::TypeMismatch { detail: "x".into() }, &[])
                .unwrap()
                .contains("Operand types differ")
        );
        assert!(
            hint(&ErrorClass::MissingKey { key: "k".into() }, &[])
                .unwrap()
                .contains("has(meta.a.k)")
        );
    }
}
