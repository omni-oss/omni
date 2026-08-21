use std::{collections::HashSet, path::PathBuf};

use derive_new::new;
use omni_core::TaskDependency;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use trace::Level;

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

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(
            level = Level::DEBUG,
            skip_all,
            fields(
                fail_fast = self.fail_fast,
                projects_count = extractions.projects.len()
            )
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

#[derive(Debug, thiserror::Error)]
#[error(
    "validation errors: \n{errors}",
    errors = format_multi_errors(errors)
)]
pub struct ExtractedDataValidationErrors {
    pub errors: Vec<ExtractedDataValidationError>,
}

impl From<Vec<ExtractedDataValidationError>> for ExtractedDataValidationErrors {
    fn from(value: Vec<ExtractedDataValidationError>) -> Self {
        Self { errors: value }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
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

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs)]
#[strum_discriminants(
    name(ExtractedDataValidationErrorKind),
    vis(pub),
    derive(strum::IntoStaticStr, strum::Display, strum::EnumIs)
)]
pub(crate) enum ExtractedDataValidationErrorInner {
    #[error(
        "duplicate project name: {project_name}\n\nprojects with same name:\n{project_paths:?}",
        project_paths = project_paths.iter().map(|p| format!("  -> {}", p.display())).collect::<Vec<_>>().join("\n")
    )]
    DuplicateProjectName {
        project_name: String,
        project_paths: Vec<PathBuf>,
    },

    #[error(
        "task '{project_name}#{task_name}' depends on unknown task '{dependency}' in project '{project_name}'"
    )]
    DanglingTaskDependency {
        project_name: String,
        task_name: String,
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
