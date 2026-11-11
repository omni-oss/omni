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
    /// Matches the task with exactly this name.
    Exact(String),
    /// Matches task names against this glob.
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
}

impl std::fmt::Debug for TaskSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(n) => write!(f, "exact:{n}"),
            Self::Pattern(p) => write!(f, "{p}"),
            Self::All => f.write_str("<all>"),
        }
    }
}
