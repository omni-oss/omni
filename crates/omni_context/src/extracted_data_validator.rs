use std::{collections::HashSet, path::PathBuf};

use derive_new::new;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

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

    pub fn validate(
        &self,
        extractions: &ProjectDataExtractions,
    ) -> Result<(), ExtractedDataValidationErrors> {
        let mut errors = vec![];

        self.validate_duplicate_project_names(extractions, &mut errors);

        if self.fail_fast && !errors.is_empty() {
            return Err(errors)?;
        }

        if errors.is_empty() {
            Ok(())
        } else {
            return Err(errors)?;
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
    errors = format_multi_errors(&errors)
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
#[error("{inner}")]
pub struct ExtractedDataValidationError {
    #[source]
    inner: ExtractedDataValidationErrorInner,
    kind: ExtractedDataValidationErrorKind,
}

impl ExtractedDataValidationError {
    pub fn kind(&self) -> ExtractedDataValidationErrorKind {
        self.kind
    }
}

impl<T: Into<ExtractedDataValidationErrorInner>> From<T>
    for ExtractedDataValidationError
{
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
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
}
