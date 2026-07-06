use std::fs;

use crate::ProjectGenerator;
use omni_configurations::{
    GeneratorSourceConfiguration, LocalGeneratorSourceConfiguration, Ui,
    WorkspaceConfiguration,
};

#[derive(Debug, bon::Builder)]
pub struct WorkspaceGenerator {
    #[builder(into)]
    name: Option<String>,
    #[builder(default)]
    projects: Vec<ProjectGenerator>,
    #[builder(default)]
    bases: Vec<ProjectGenerator>,
}

impl WorkspaceGenerator {
    pub fn generate(
        &self,
        workspace_dir: impl AsRef<std::path::Path>,
    ) -> eyre::Result<()> {
        let workspace_dir = workspace_dir.as_ref();

        let ws = WorkspaceConfiguration {
            projects: vec!["./projects/*".to_string()],
            generators: vec![GeneratorSourceConfiguration::new_local(
                LocalGeneratorSourceConfiguration::new(
                    "./generators/*".to_string(),
                ),
            )],
            name: self.name.clone(),
            env: Default::default(),
            ui: Ui::Stream,
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

        let mut file = std::io::BufWriter::new(file);

        omni_file_data_serde::to_writer(
            &mut file,
            &ws,
            omni_file_data_serde::Format::Yaml,
        )?;
        Ok(())
    }
}
