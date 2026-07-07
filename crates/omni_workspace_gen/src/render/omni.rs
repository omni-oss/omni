//! Pure renderer for the omni configuration layer.
//!
//! Turns a [`WorkspaceModel`] into omni config file *contents* (never touching
//! the filesystem): a `workspace.omni.yaml` plus one `project.omni.yaml` per
//! project. Host-specific bits — how a task is invoked and which files feed its
//! cache key — are supplied via [`OmniRenderOptions`], so the Rust bench host
//! and the TypeScript task-bench host share one renderer while keeping their
//! different neutral bases (launcher script vs `task.mjs`).

use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};

use crate::WorkspaceModel;

const WORKSPACE_SCHEMA: &str = "# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/json-schemas/refs/heads/main/workspace.json";
const PROJECT_SCHEMA: &str = "# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/json-schemas/refs/heads/main/project.json";

/// Placeholder replaced by the task name (e.g. `t0`) in
/// [`OmniRenderOptions::task_command_template`].
const TASK_ID_PLACEHOLDER: &str = "{task_id}";

/// Host-specific inputs for rendering the omni layer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct OmniRenderOptions {
    /// Command template for each task's `exec`. `{task_id}` is replaced by the
    /// task name, e.g. `"node ./task.mjs {task_id}"` or `"sh run.sh {task_id}"`.
    pub task_command_template: String,
    /// Per-project cache-key input file globs (host-specific, since the neutral
    /// bases differ), e.g. `["./src/**/*.*"]` or `["package.json", "task.mjs"]`.
    pub project_cache_key_files: Vec<String>,
}

/// Render the omni-layer files for `model` as `(workspace-relative path,
/// contents)`. The caller writes them; this never touches disk.
pub fn render_omni(
    model: &WorkspaceModel,
    options: &OmniRenderOptions,
) -> Vec<(String, String)> {
    let cache_enabled = model.config.cache_enabled;

    let mut files = Vec::with_capacity(model.projects.len() + 1);

    files.push((
        "workspace.omni.yaml".to_string(),
        render_doc(WORKSPACE_SCHEMA, &workspace_doc()),
    ));

    for project in &model.projects {
        let tasks = project
            .tasks
            .iter()
            .map(|task| {
                let exec = options
                    .task_command_template
                    .replace(TASK_ID_PLACEHOLDER, &task.name);
                let cache = cache_enabled.then(|| TaskCacheDoc {
                    output: FilesDoc {
                        files: task.output_globs.clone(),
                    },
                });
                (
                    task.name.clone(),
                    TaskDoc {
                        exec,
                        dependencies: task.dependencies.clone(),
                        cache,
                    },
                )
            })
            .collect::<Vec<_>>();

        let cache = cache_enabled.then(|| ProjectCacheDoc {
            key: FilesDoc {
                files: options.project_cache_key_files.clone(),
            },
        });

        let doc = ProjectDoc {
            name: project.name.clone(),
            dependencies: project.dependencies.clone(),
            cache,
            tasks: OrderedTasks(tasks),
        };

        files.push((
            format!("{}/project.omni.yaml", project.dir),
            render_doc(PROJECT_SCHEMA, &doc),
        ));
    }

    files
}

fn workspace_doc() -> WorkspaceDoc {
    WorkspaceDoc {
        ui: "stream",
        projects: vec!["packages/*".to_string()],
    }
}

fn render_doc<T: Serialize>(schema: &str, doc: &T) -> String {
    let mut buf = Vec::new();
    noyalib::to_writer(&mut buf, doc)
        .expect("omni config serialization is infallible for plain structs");
    let body = String::from_utf8(buf)
        .expect("noyalib emits valid UTF-8 for string/scalar content");
    format!("{schema}\n{body}")
}

#[derive(Serialize)]
struct WorkspaceDoc {
    ui: &'static str,
    projects: Vec<String>,
}

#[derive(Serialize)]
struct ProjectDoc {
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache: Option<ProjectCacheDoc>,
    tasks: OrderedTasks,
}

#[derive(Serialize)]
struct ProjectCacheDoc {
    key: FilesDoc,
}

#[derive(Serialize)]
struct TaskDoc {
    exec: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache: Option<TaskCacheDoc>,
}

#[derive(Serialize)]
struct TaskCacheDoc {
    output: FilesDoc,
}

#[derive(Serialize)]
struct FilesDoc {
    files: Vec<String>,
}

/// A YAML mapping of task name -> task, serialized in insertion order (so tasks
/// read `t0`, `t1`, ... rather than a lexicographic sort) without pulling in an
/// ordered-map dependency.
struct OrderedTasks(Vec<(String, TaskDoc)>);

impl Serialize for OrderedTasks {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, task) in &self.0 {
            map.serialize_entry(name, task)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DependencyConfig, DependencyStrategy, HarnessConfig, build_model,
    };

    fn options() -> OmniRenderOptions {
        OmniRenderOptions {
            task_command_template: "node ./task.mjs {task_id}".to_string(),
            project_cache_key_files: vec![
                "package.json".to_string(),
                "task.mjs".to_string(),
                "src/**/*.js".to_string(),
            ],
        }
    }

    fn model() -> WorkspaceModel {
        build_model(
            &HarnessConfig::builder()
                .projects(3)
                .tasks_per_project(2)
                .dependency(
                    DependencyConfig::builder()
                        .strategy(DependencyStrategy::Chain)
                        .build(),
                )
                .build(),
        )
    }

    #[test]
    fn renders_workspace_and_one_file_per_project() {
        let files = render_omni(&model(), &options());
        let paths = files.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>();

        assert_eq!(paths[0], "workspace.omni.yaml");
        assert!(paths.contains(&"packages/p-0/project.omni.yaml"));
        assert!(paths.contains(&"packages/p-1/project.omni.yaml"));
        assert!(paths.contains(&"packages/p-2/project.omni.yaml"));
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn workspace_file_declares_packages_glob() {
        let files = render_omni(&model(), &options());
        let (_, body) = &files[0];
        assert!(body.contains("workspace.json"));
        assert!(body.contains("packages/*"));
        assert!(body.contains("ui: stream"));
    }

    #[test]
    fn project_file_applies_command_template_and_edges() {
        let files = render_omni(&model(), &options());
        let (_, body) = files
            .iter()
            .find(|(p, _)| p == "packages/p-1/project.omni.yaml")
            .unwrap();

        // Command template expanded per task.
        assert!(body.contains("node ./task.mjs t0"));
        assert!(body.contains("node ./task.mjs t1"));
        // Resolved upstream + intra edges.
        assert!(body.contains("^t0"));
        // Upstream project dependency resolved to a name.
        assert!(body.contains("p-0"));
        // Per-task output glob + project cache key inputs.
        assert!(body.contains("dist/t0.*"));
        assert!(body.contains("package.json"));
    }

    #[test]
    fn cache_omitted_when_disabled() {
        let cfg = HarnessConfig::builder()
            .projects(2)
            .tasks_per_project(1)
            .cache_enabled(false)
            .build();
        let files = render_omni(&build_model(&cfg), &options());
        let (_, body) = files
            .iter()
            .find(|(p, _)| p.ends_with("project.omni.yaml"))
            .unwrap();

        assert!(!body.contains("cache"));
    }

    #[test]
    fn render_is_byte_stable() {
        let m = model();
        let opts = options();
        assert_eq!(render_omni(&m, &opts), render_omni(&m, &opts));
    }
}
