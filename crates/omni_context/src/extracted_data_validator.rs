use std::{collections::HashSet, path::PathBuf};

use derive_new::new;
use omni_core::TaskDependency;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use tracing::Level;

use crate::project_data_extractor::ProjectDataExtractions;

#[derive(Debug, Default, new)]
pub struct ExtractedDataValidator {
    fail_fast: bool,
}

impl ExtractedDataValidator {
    fn validate_duplicate_project_names(
        &self,
        extractions: &ProjectDataExtractions,
        errors: &mut Vec<ExtractedDataValidationError>,
    ) {
        // check duplicate names
        let mut names = HashSet::new();
        for project in &extractions.projects {
            if names.contains(&project.name) {
                let paths = extractions
                    .projects
                    .iter()
                    .filter_map(|p| {
                        if *p.name == *project.name {
                            Some(p.dir.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                let error =
                    ExtractedDataValidationErrorInner::DuplicateProjectName {
                        project_name: project.name.clone(),
                        project_paths: paths,
                    };

                errors.push(error.into());

                if self.fail_fast {
                    break;
                }
            }

            names.insert(project.name.clone());
        }
    }

    fn validate_dangling_own_dependencies(
        &self,
        extractions: &ProjectDataExtractions,
        errors: &mut Vec<ExtractedDataValidationError>,
    ) {
        for project in &extractions.projects {
            for (task_name, task) in &project.tasks {
                let references =
                    task.dependencies.iter().chain(task.siblings.iter());

                for reference in references {
                    let TaskDependency::Own { task: dependency } = reference
                    else {
                        continue;
                    };

                    if !project.tasks.contains_key(dependency) {
                        errors.push(
                            ExtractedDataValidationErrorInner::DanglingTaskDependency {
                                project_name: project.name.clone(),
                                task_name: task_name.clone(),
                                dependency: dependency.clone(),
                            }
                            .into(),
                        );

                        if self.fail_fast {
                            return;
                        }
                    }
                }
            }
        }
    }

    #[tracing::instrument(
            level = Level::DEBUG,
            skip_all,
            fields(
                fail_fast = self.fail_fast,
                projects_count = extractions.projects.len()
            )
        )]
    pub fn validate(
        &self,
        extractions: &ProjectDataExtractions,
    ) -> Result<(), ExtractedDataValidationErrors> {
        let mut errors = vec![];

        self.validate_duplicate_project_names(extractions, &mut errors);

        if self.fail_fast && !errors.is_empty() {
            return Err(errors.into());
        }

        self.validate_dangling_own_dependencies(extractions, &mut errors);

        if self.fail_fast && !errors.is_empty() {
            return Err(errors.into());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)?
        }
    }
}

fn aggregate_details(
    errors: &ExtractedDataValidationErrors,
) -> serde_json::Value {
    use omni_diagnostic::Diagnostic as _;

    serde_json::json!({
        "errors": errors
            .errors
            .iter()
            .map(|e| serde_json::json!({
                "code": e.code(),
                "message": e.to_string(),
                "details": e.details(),
            }))
            .collect::<Vec<_>>(),
    })
}

fn aggregate_hints(errors: &ExtractedDataValidationErrors) -> Vec<String> {
    use omni_diagnostic::Diagnostic as _;

    let mut hints: Vec<String> = Vec::new();
    for hint in errors.errors.iter().flat_map(|e| e.hints()) {
        if !hints.contains(&hint) {
            hints.push(hint);
        }
    }
    hints
}

fn aggregate_body(
    errors: &ExtractedDataValidationErrors,
    out: &mut dyn std::fmt::Write,
) -> std::fmt::Result {
    use omni_diagnostic::Diagnostic as _;

    for e in &errors.errors {
        let message = e.to_string();
        let mut lines = message.lines();
        if let Some(first) = lines.next() {
            writeln!(out, "  - [{}] {first}", e.code())?;
        }
        for line in lines {
            writeln!(out, "    {line}")?;
        }
    }
    Ok(())
}

fn digits(n: usize) -> usize {
    (n as f64).log10().ceil() as usize
}

fn format_multi_errors(errors: &[ExtractedDataValidationError]) -> String {
    let digits = digits(errors.len());
    let mut lines = Vec::with_capacity(errors.len());

    for (i, error) in errors.iter().enumerate() {
        let err_string = error.to_string();
        let error_lines = err_string.split('\n').enumerate();

        for (j, line) in error_lines {
            if j == 0 {
                lines.push(format!(
                    "{i:>width$}. {line}",
                    i = i,
                    width = digits
                ));
            } else {
                lines.push(format!("{}  {line}", " ".repeat(digits)));
            }
        }
    }

    lines.join("\n")
}

/// Workspace validation failed.
///
/// One or more projects or tasks are invalid. `details.errors` lists every
/// failure with its own code, message and details.
#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(
    "validation errors: \n{errors}",
    errors = format_multi_errors(errors)
)]
#[diagnostic(
    code = "workspace.validation_failed",
    details = aggregate_details,
    hints = aggregate_hints,
    body = aggregate_body
)]
pub struct ExtractedDataValidationErrors {
    pub errors: Vec<ExtractedDataValidationError>,
}

impl From<Vec<ExtractedDataValidationError>> for ExtractedDataValidationErrors {
    fn from(value: Vec<ExtractedDataValidationError>) -> Self {
        Self { errors: value }
    }
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct ExtractedDataValidationError(
    pub(crate) ExtractedDataValidationErrorInner,
);

impl ExtractedDataValidationError {
    #[allow(unused)]
    pub fn kind(&self) -> ExtractedDataValidationErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ExtractedDataValidationErrorInner>> From<T>
    for ExtractedDataValidationError
{
    fn from(value: T) -> Self {
        let repr = value.into();
        Self(repr)
    }
}

#[derive(
    Debug,
    thiserror::Error,
    EnumDiscriminants,
    EnumIs,
    omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(
    name(ExtractedDataValidationErrorKind),
    vis(pub),
    derive(strum::IntoStaticStr, strum::Display, strum::EnumIs)
)]
pub(crate) enum ExtractedDataValidationErrorInner {
    /// Two projects share the same name.
    #[error(
        "duplicate project name: {project_name}\n\nprojects with same name:\n{project_paths:?}",
        project_paths = project_paths.iter().map(|p| format!("  -> {}", p.display())).collect::<Vec<_>>().join("\n")
    )]
    #[diagnostic(
        code = "workspace.duplicate_project_name",
        hint = "rename one of the projects called `{project_name}` so project names are unique"
    )]
    DuplicateProjectName {
        #[diagnostic(detail = "project")]
        project_name: String,
        #[diagnostic(detail = "paths")]
        project_paths: Vec<PathBuf>,
    },

    /// A task depends on a task that its own project does not define.
    #[error(
        "task '{project_name}#{task_name}' depends on unknown task '{dependency}' in project '{project_name}'"
    )]
    #[diagnostic(
        code = "workspace.dangling_task_dependency",
        hint = "define a task named `{dependency}` in project `{project_name}` or remove the dependency"
    )]
    DanglingTaskDependency {
        #[diagnostic(detail = "project")]
        project_name: String,
        #[diagnostic(detail = "task")]
        task_name: String,
        #[diagnostic(detail)]
        dependency: String,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use omni_core::{Project, Task, TaskDependency};

    use super::*;
    use crate::project_data_extractor::ProjectDataExtractions;

    fn task_with_own_deps(deps: &[&str]) -> Task {
        Task::new(
            None,
            None,
            deps.iter()
                .map(|d| TaskDependency::Own {
                    task: d.to_string(),
                })
                .collect(),
            None,
            true.into(),
            false,
            false,
            vec![],
            None,
            None,
        )
    }

    fn project(name: &str, tasks: Vec<(&str, Task)>) -> Project {
        let mut task_map = maps::OrderedMap::new();
        for (task_name, task) in tasks {
            task_map.insert(task_name.to_string(), task);
        }

        Project::new(name, PathBuf::from("/tmp"), vec![], task_map)
    }

    fn extractions(projects: Vec<Project>) -> ProjectDataExtractions {
        ProjectDataExtractions::new(
            projects,
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
    }

    #[test]
    fn test_dangling_own_dependency_is_error() {
        let ex = extractions(vec![project(
            "app",
            vec![("build", task_with_own_deps(&["helper"]))],
        )]);

        let err = ExtractedDataValidator::new(false)
            .validate(&ex)
            .expect_err("dangling own dependency must fail validation");

        assert_eq!(err.errors.len(), 1);
        assert_eq!(
            err.errors[0].kind(),
            ExtractedDataValidationErrorKind::DanglingTaskDependency
        );
    }

    #[test]
    fn test_valid_own_dependency_passes() {
        let ex = extractions(vec![project(
            "app",
            vec![
                ("build", task_with_own_deps(&["helper"])),
                ("helper", task_with_own_deps(&[])),
            ],
        )]);

        ExtractedDataValidator::new(false)
            .validate(&ex)
            .expect("valid own dependency must pass validation");
    }
}
