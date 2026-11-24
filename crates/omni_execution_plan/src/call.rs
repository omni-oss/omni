use derive_new::new;
use strum::{Display, EnumIs};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, new, Display, EnumIs,
)]
pub enum Call {
    #[strum(to_string = "command '{command} {args:?}'")]
    Command {
        #[new(into)]
        command: String,
        #[new(into)]
        args: Vec<String>,
    },

    #[strum(to_string = "tasks '{0:?}'")]
    Tasks(#[new(into)] Vec<TaskSelector>),
}

impl Call {
    /// Tasks selected by glob pattern. An empty list applies no task filter.
    pub fn pattern_tasks<S: Into<String>>(
        patterns: impl IntoIterator<Item = S>,
    ) -> Self {
        Self::Tasks(patterns.into_iter().map(TaskSelector::pattern).collect())
    }

    /// Tasks selected by exact name; glob characters are literal.
    pub fn exact_tasks<S: Into<String>>(
        names: impl IntoIterator<Item = S>,
    ) -> Self {
        Self::Tasks(names.into_iter().map(TaskSelector::exact).collect())
    }

    /// Every task, regardless of name.
    pub fn all_tasks() -> Self {
        Self::Tasks(vec![TaskSelector::All])
    }
}

/// How a task request picks task names. `Exact` and `Pattern` are separate
/// paths so a name containing glob characters can be selected literally.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskSelector {
    /// Matches the task whose canonical name is exactly this. This is the raw
    /// stored name: aliases are not consulted. Inventory and internally
    /// generated names (such as `exec`'s synthetic task) use it.
    Exact(String),
    /// A parsed live exact selector: an optional project qualifier and a task
    /// name that may be a canonical name or an alias of the project that owns
    /// the task. Glob characters are literal.
    Reference {
        project: Option<String>,
        task: String,
    },
    /// Matches task names (canonical names and aliases) against this glob.
    Pattern(String),
    /// Matches every task.
    All,
}

impl TaskSelector {
    pub fn exact(name: impl Into<String>) -> Self {
        Self::Exact(name.into())
    }

    pub fn pattern(glob: impl Into<String>) -> Self {
        Self::Pattern(glob.into())
    }

    /// Parse live exact selector input once, at ingress, with the shared
    /// reference grammar. `project#task` scopes to one project; `\#`, `\:` and
    /// `\\` write literal characters; wildcard characters are literal. Plugin
    /// qualification and a leading `^` are rejected.
    pub fn parse_exact(input: &str) -> Result<Self, ExactSelectorError> {
        use omni_types::{ReferenceSurface, TaskReference};
        let fail = |message: String| ExactSelectorError {
            input: input.to_owned(),
            message,
        };
        let reference =
            TaskReference::parse(input).map_err(|e| fail(e.to_string()))?;
        reference
            .validate_for(ReferenceSurface::ExactSelector)
            .map_err(|e| fail(e.to_string()))?;
        if reference.plugin().is_some() {
            return Err(fail(
                "plugin scopes are not supported in this version; escape a literal `::` with `\\:\\:`"
                    .to_owned(),
            ));
        }
        Ok(Self::Reference {
            project: reference.project().map(str::to_owned),
            task: reference.task().to_owned(),
        })
    }
}

/// A live exact task selector that does not parse or is not allowed there.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid task selector `{input}`: {message}")]
pub struct ExactSelectorError {
    pub input: String,
    pub message: String,
}

impl std::fmt::Debug for TaskSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(n) => write!(f, "exact:{n}"),
            Self::Reference { project, task } => match project {
                Some(project) => {
                    write!(f, "exact:{}", omni_types::task_label(project, task))
                }
                None => write!(f, "exact:{task}"),
            },
            Self::Pattern(p) => write!(f, "{p}"),
            Self::All => f.write_str("<all>"),
        }
    }
}

/// How a project filter picks project names. `Exact` and `Pattern` are
/// separate paths so a name containing glob characters can be selected
/// literally. No selectors means every project.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProjectSelector {
    /// Matches the project with exactly this name.
    Exact(String),
    /// Matches project names against this glob.
    Pattern(String),
}

impl ProjectSelector {
    pub fn exact(name: impl Into<String>) -> Self {
        Self::Exact(name.into())
    }

    pub fn pattern(glob: impl Into<String>) -> Self {
        Self::Pattern(glob.into())
    }
}

impl std::fmt::Debug for ProjectSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(n) => write!(f, "exact:{n}"),
            Self::Pattern(p) => write!(f, "{p}"),
        }
    }
}

impl std::fmt::Display for ProjectSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(n) => f.write_str(n),
            Self::Pattern(p) => write!(f, "{p} (pattern)"),
        }
    }
}
