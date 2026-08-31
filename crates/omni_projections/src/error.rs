use std::fmt;
use std::path::PathBuf;

use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ProjectionError(pub(crate) ProjectionErrorInner);

impl ProjectionError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(ProjectionErrorInner::Custom(eyre::Report::msg(
            message.into(),
        )))
    }

    pub fn conflicts(report: ConflictReport) -> Self {
        Self(ProjectionErrorInner::Conflicts(report))
    }

    pub fn kind(&self) -> ProjectionErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ProjectionErrorInner>> From<T> for ProjectionError {
    fn from(inner: T) -> Self {
        Self(inner.into())
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ProjectionErrorKind))]
pub(crate) enum ProjectionErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Hasher(#[from] omni_hasher::HasherError),

    #[error("{0}")]
    Conflicts(ConflictReport),
}

/// One destination claimed by more than one distinct planned link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateDest {
    pub dest: PathBuf,
    pub sources: Vec<PathBuf>,
}

/// A destination that lies strictly inside another planned directory-link
/// destination. Writing the inner link would land back inside the source the
/// outer link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedDest {
    pub inner: PathBuf,
    pub outer: PathBuf,
}

/// A foreign file already present at a destination whose policy is `error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingConflict {
    pub dest: PathBuf,
}

/// Every conflict found by the whole-run preflight, collected so a single error
/// reports all of them instead of failing on the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConflictReport {
    pub duplicate_dests: Vec<DuplicateDest>,
    pub nested: Vec<NestedDest>,
    pub existing: Vec<ExistingConflict>,
}

impl ConflictReport {
    pub fn is_empty(&self) -> bool {
        self.duplicate_dests.is_empty()
            && self.nested.is_empty()
            && self.existing.is_empty()
    }

    pub fn total(&self) -> usize {
        self.duplicate_dests.len() + self.nested.len() + self.existing.len()
    }
}

impl fmt::Display for ConflictReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "projection sync aborted: {} conflict(s) found, nothing was written",
            self.total()
        )?;
        for dup in &self.duplicate_dests {
            let sources: Vec<String> = dup
                .sources
                .iter()
                .map(|s| s.display().to_string())
                .collect();
            writeln!(
                f,
                "  collision: {} is claimed by multiple links (sources: {})",
                dup.dest.display(),
                sources.join(", ")
            )?;
        }
        for nested in &self.nested {
            writeln!(
                f,
                "  collision: {} is nested inside directory link {}",
                nested.inner.display(),
                nested.outer.display()
            )?;
        }
        for existing in &self.existing {
            writeln!(
                f,
                "  existing file: {} already exists (set on_existing to backup, overwrite, or skip)",
                existing.dest.display()
            )?;
        }
        Ok(())
    }
}

pub type Result<T, E = ProjectionError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflicts_kind_and_counts_are_preserved() {
        let report = ConflictReport {
            duplicate_dests: vec![DuplicateDest {
                dest: PathBuf::from("/ws/out/a"),
                sources: vec![
                    PathBuf::from("/src/one/a"),
                    PathBuf::from("/src/two/a"),
                ],
            }],
            nested: vec![NestedDest {
                inner: PathBuf::from("/ws/out/dir/inner"),
                outer: PathBuf::from("/ws/out/dir"),
            }],
            existing: vec![ExistingConflict {
                dest: PathBuf::from("/ws/out/b"),
            }],
        };
        assert_eq!(report.total(), 3);

        let err = ProjectionError::conflicts(report);
        assert_eq!(err.kind(), ProjectionErrorKind::Conflicts);
    }

    #[test]
    fn conflicts_display_lists_each_on_its_own_line() {
        let report = ConflictReport {
            duplicate_dests: vec![DuplicateDest {
                dest: PathBuf::from("/ws/out/a"),
                sources: vec![PathBuf::from("/src/one/a")],
            }],
            nested: vec![NestedDest {
                inner: PathBuf::from("/ws/out/dir/inner"),
                outer: PathBuf::from("/ws/out/dir"),
            }],
            existing: vec![ExistingConflict {
                dest: PathBuf::from("/ws/out/b"),
            }],
        };
        let rendered = report.to_string();
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 4, "header plus one line per conflict");
        assert!(lines[0].contains("3 conflict"));
        assert!(lines[1].contains("collision"));
        assert!(lines[2].contains("nested"));
        assert!(lines[3].contains("existing file"));
    }

    #[test]
    fn empty_report_is_empty() {
        assert!(ConflictReport::default().is_empty());
    }
}
