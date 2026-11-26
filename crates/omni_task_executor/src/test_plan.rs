//! A planned `with` sibling whose prerequisite is not selected by the filter,
//! built through the real planner so caller tests see what a run would see.

use std::path::PathBuf;

use maps::OrderedMap;
use omni_command_config::CommandConfig;
use omni_config_types::TeraExprBoolean;
use omni_core::{
    BatchedExecutionPlan, Project, ProjectGraph, Task, TaskDependency,
};

fn task(deps: &[&str], siblings: &[&str]) -> Task {
    let own = |names: &[&str]| {
        names
            .iter()
            .map(|t| TaskDependency::Own { task: t.to_string() })
            .collect::<Vec<_>>()
    };
    Task::new(
        Some(CommandConfig::Shell("x".to_string())),
        None,
        own(deps),
        None,
        TeraExprBoolean::Boolean(true),
        false,
        false,
        own(siblings),
        Default::default(),
    )
}

/// `app#t` has `with: [tool#build-native]`, `tool#build-native` depends on
/// `tool#setup`, and the filter selects only the project `app`.
pub(crate) fn app_plan() -> BatchedExecutionPlan {
    let mut app_tasks = OrderedMap::default();
    app_tasks.insert("t".to_string(), {
        let mut t = task(&[], &[]);
        t.siblings.push(TaskDependency::ExplicitProject {
            project: "tool".to_string(),
            task: "build-native".to_string(),
        });
        t
    });
    let mut tool_tasks = OrderedMap::default();
    tool_tasks.insert("setup".to_string(), task(&[], &[]));
    tool_tasks
        .insert("build-native".to_string(), task(&["setup"], &[]));
    let graph = ProjectGraph::from_projects(vec![
        Project::new(
            "app",
            PathBuf::new(),
            vec!["tool".to_string()],
            app_tasks,
        ),
        Project::new("tool", PathBuf::new(), vec![], tool_tasks),
    ])
    .unwrap()
    .get_task_execution_graph()
    .unwrap();
    graph
        .get_batched_execution_plan(|n| Ok(n.project_name() == "app"))
        .unwrap()
}
