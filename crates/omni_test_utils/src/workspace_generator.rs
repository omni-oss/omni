use std::fs;

use derive_builder::Builder;

use crate::{ProjectGenerator, ProjectGeneratorBuilder};
use omni_configurations::{ExecutorsConfiguration, WorkspaceConfiguration};

#[derive(Builder, Debug)]
#[builder(setter(into, strip_option))]
pub struct WorkspaceGenerator {
    name: Option<String>,
    #[builder(setter(custom), default)]
    projects: Vec<ProjectGenerator>,
    #[builder(setter(custom), default)]
    bases: Vec<ProjectGenerator>,
}

impl WorkspaceGeneratorBuilder {
    fn add_project(
        projects: &mut Option<Vec<ProjectGenerator>>,
        f: impl FnOnce(&mut ProjectGeneratorBuilder) -> eyre::Result<()>,
        base: Option<bool>,
    ) -> eyre::Result<()> {
        let mut project = ProjectGeneratorBuilder::default();
        f(&mut project)?;

        if let Some(base) = base {
            project.base(base);
        }

        if let Some(projects) = projects {
            projects.push(project.build()?);
        } else {
            *projects = Some(vec![project.build()?]);
        }

        Ok(())
    }

    pub fn project(
        &mut self,
        f: impl FnOnce(&mut ProjectGeneratorBuilder) -> eyre::Result<()>,
    ) -> eyre::Result<&mut Self> {
        Self::add_project(&mut self.projects, f, None)?;

        Ok(self)
    }

    pub fn base(
        &mut self,
        f: impl FnOnce(&mut ProjectGeneratorBuilder) -> eyre::Result<()>,
    ) -> eyre::Result<&mut Self> {
        Self::add_project(&mut self.bases, f, Some(true))?;

        Ok(self)
    }
}

impl WorkspaceGenerator {
    pub fn builder() -> WorkspaceGeneratorBuilder {
        WorkspaceGeneratorBuilder::default()
    }
}

impl WorkspaceGenerator {
    pub fn generate(
        &self,
        workspace_dir: impl AsRef<std::path::Path>,
    ) -> eyre::Result<()> {
        let workspace_dir = workspace_dir.as_ref();

        let ws = WorkspaceConfiguration {
            projects: vec!["./projects/*".to_string()],
            generators: vec!["./generators/*".to_string()],
            executors: ExecutorsConfiguration::default(),
            name: self.name.clone(),
            env: Default::default(),
        };

        fs::create_dir_all(workspace_dir.join("projects"))?;
        fs::create_dir_all(workspace_dir.join("bases"))?;
        fs::create_dir_all(workspace_dir.join("generators"))?;

        // ignore all base configurations
        fs::write(workspace_dir.join(".omniignore"), r#"bases/**/*.omni.yml"#)?;

        for base in &self.bases {
            let dir_name = bs58::encode(base.name.as_bytes()).into_string();

            base.generate(workspace_dir.join("bases").join(dir_name))?;
        }

        for project in &self.projects {
            let dir_name = bs58::encode(project.name.as_bytes()).into_string();

            project.generate(workspace_dir.join("projects").join(dir_name))?;
        }

        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(workspace_dir.join("workspace.omni.yml"))?;

        let file = std::io::BufWriter::new(file);
        serde_yml::to_writer(file, &ws)?;

        Ok(())
    }
}
