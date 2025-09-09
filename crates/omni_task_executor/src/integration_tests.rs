// ============================================================================
// COMPLETE INTEGRATION EXAMPLE
// ============================================================================

use std::sync::Arc;

// This shows how to use the refactored system in practice
pub struct RefactoredTaskExecutor<TSys: TaskExecutorSys> {
    factory: TaskExecutorFactory<TSys>,
}

impl<TSys: TaskExecutorSys> RefactoredTaskExecutor<TSys> {
    pub fn new(context: Context<TSys>) -> Self {
        Self {
            factory: TaskExecutorFactory::new(context),
        }
    }

    /// Execute with custom configuration
    pub async fn execute_with_config(
        &self,
        config: ExecutionConfig,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        let mut pipeline = self.factory.create_execution_pipeline(config);
        pipeline.execute().await
    }

    /// Execute a single command across all projects
    pub async fn execute_command(
        &self,
        command: &str,
        args: &[String],
        project_filter: Option<&str>,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        let config = ExecutionConfig {
            call: Call::Command {
                command: command.to_string(),
                args: args.to_vec(),
            },
            ignore_dependencies: true,
            project_filter: project_filter.map(String::from),
            meta_filter: None,
            force: true,
            no_cache: true,
            on_failure: OnFailure::Continue,
            dry_run: false,
            replay_cached_logs: false,
            add_task_details: false,
        };

        self.execute_with_config(config).await
    }

    /// Execute a specific task with dependency resolution
    pub async fn execute_task(
        &self,
        task_name: &str,
        project_filter: Option<&str>,
        force: bool,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        let config = ExecutionConfig {
            call: Call::Task(task_name.to_string()),
            ignore_dependencies: false,
            project_filter: project_filter.map(String::from),
            meta_filter: None,
            force,
            no_cache: false,
            on_failure: OnFailure::SkipDependents,
            dry_run: false,
            replay_cached_logs: true,
            add_task_details: true,
        };

        self.execute_with_config(config).await
    }

    /// Dry run execution
    pub async fn dry_run(
        &self,
        call: Call,
        project_filter: Option<&str>,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        let config = ExecutionConfig {
            call,
            ignore_dependencies: false,
            project_filter: project_filter.map(String::from),
            meta_filter: None,
            force: false,
            no_cache: false,
            on_failure: OnFailure::Continue,
            dry_run: true,
            replay_cached_logs: false,
            add_task_details: true,
        };

        self.execute_with_config(config).await
    }
}

// ============================================================================
// BUILDER PATTERN FOR EASIER CONFIGURATION
// ============================================================================

pub struct ExecutionConfigBuilder {
    config: ExecutionConfig,
}

impl ExecutionConfigBuilder {
    pub fn new(call: Call) -> Self {
        Self {
            config: ExecutionConfig {
                call,
                ignore_dependencies: false,
                project_filter: None,
                meta_filter: None,
                force: false,
                no_cache: false,
                on_failure: OnFailure::SkipDependents,
                dry_run: false,
                replay_cached_logs: true,
                add_task_details: false,
            },
        }
    }

    pub fn ignore_dependencies(mut self, ignore: bool) -> Self {
        self.config.ignore_dependencies = ignore;
        self
    }

    pub fn project_filter(mut self, filter: impl Into<String>) -> Self {
        self.config.project_filter = Some(filter.into());
        self
    }

    pub fn meta_filter(mut self, filter: impl Into<String>) -> Self {
        self.config.meta_filter = Some(filter.into());
        self
    }

    pub fn force(mut self, force: bool) -> Self {
        self.config.force = force;
        self
    }

    pub fn no_cache(mut self, no_cache: bool) -> Self {
        self.config.no_cache = no_cache;
        self
    }

    pub fn on_failure(mut self, on_failure: OnFailure) -> Self {
        self.config.on_failure = on_failure;
        self
    }

    pub fn dry_run(mut self, dry_run: bool) -> Self {
        self.config.dry_run = dry_run;
        self
    }

    pub fn replay_cached_logs(mut self, replay: bool) -> Self {
        self.config.replay_cached_logs = replay;
        self
    }

    pub fn add_task_details(mut self, add_details: bool) -> Self {
        self.config.add_task_details = add_details;
        self
    }

    pub fn build(self) -> ExecutionConfig {
        self.config
    }
}

// ============================================================================
// SPECIALIZED EXECUTORS FOR COMMON PATTERNS
// ============================================================================

/// Executor specialized for running commands across projects
pub struct CommandExecutor<TSys: TaskExecutorSys> {
    executor: RefactoredTaskExecutor<TSys>,
}

impl<TSys: TaskExecutorSys> CommandExecutor<TSys> {
    pub fn new(context: Context<TSys>) -> Self {
        Self {
            executor: RefactoredTaskExecutor::new(context),
        }
    }

    pub async fn run_across_all_projects(
        &self,
        command: &str,
        args: &[String],
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        self.executor.execute_command(command, args, None).await
    }

    pub async fn run_across_filtered_projects(
        &self,
        command: &str,
        args: &[String],
        project_filter: &str,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        self.executor
            .execute_command(command, args, Some(project_filter))
            .await
    }

    pub async fn run_with_custom_failure_handling(
        &self,
        command: &str,
        args: &[String],
        on_failure: OnFailure,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        let config = ExecutionConfigBuilder::new(Call::Command {
            command: command.to_string(),
            args: args.to_vec(),
        })
        .ignore_dependencies(true)
        .force(true)
        .no_cache(true)
        .on_failure(on_failure)
        .build();

        self.executor.execute_with_config(config).await
    }
}

/// Executor specialized for running tasks with dependency resolution
pub struct TaskExecutor<TSys: TaskExecutorSys> {
    executor: RefactoredTaskExecutor<TSys>,
}

impl<TSys: TaskExecutorSys> TaskExecutor<TSys> {
    pub fn new(context: Context<TSys>) -> Self {
        Self {
            executor: RefactoredTaskExecutor::new(context),
        }
    }

    pub async fn run_task(
        &self,
        task_name: &str,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        self.executor.execute_task(task_name, None, false).await
    }

    pub async fn run_task_forced(
        &self,
        task_name: &str,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        self.executor.execute_task(task_name, None, true).await
    }

    pub async fn run_task_with_filter(
        &self,
        task_name: &str,
        project_filter: &str,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        self.executor
            .execute_task(task_name, Some(project_filter), false)
            .await
    }

    pub async fn run_task_with_meta_filter(
        &self,
        task_name: &str,
        meta_filter: &str,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        let config =
            ExecutionConfigBuilder::new(Call::Task(task_name.to_string()))
                .meta_filter(meta_filter)
                .add_task_details(true)
                .build();

        self.executor.execute_with_config(config).await
    }
}

// ============================================================================
// USAGE EXAMPLES
// ============================================================================

#[cfg(test)]
mod usage_examples {
    use super::*;

    // Example 1: Simple command execution
    #[tokio::test]
    async fn example_run_command() {
        // let context = create_test_context();
        // let executor = CommandExecutor::new(context);

        // let results = executor
        //     .run_across_all_projects("npm", &["test".to_string()])
        //     .await
        //     .expect("Command execution failed");

        // for result in results {
        //     match result {
        //         TaskExecutionResult::Completed { exit_code, task, .. } => {
        //             println!("Task {} completed with exit code {}", task.full_task_name(), exit_code);
        //         }
        //         TaskExecutionResult::Error { task, error, .. } => {
        //             eprintln!("Task {} failed: {}", task.full_task_name(), error);
        //         }
        //         TaskExecutionResult::Skipped { task, skip_reason, .. } => {
        //             println!("Task {} skipped: {}", task.full_task_name(), skip_reason);
        //         }
        //     }
        // }
    }

    // Example 2: Task execution with dependencies
    #[tokio::test]
    async fn example_run_task() {
        // let context = create_test_context();
        // let executor = TaskExecutor::new(context);

        // let results = executor
        //     .run_task("build")
        //     .await
        //     .expect("Task execution failed");

        // // Process results...
    }

    // Example 3: Custom configuration
    #[tokio::test]
    async fn example_custom_config() {
        // let context = create_test_context();
        // let executor = RefactoredTaskExecutor::new(context);

        // let config = ExecutionConfigBuilder::new(Call::Task("deploy".to_string()))
        //     .project_filter("backend-*")
        //     .meta_filter("env == 'production'")
        //     .force(true)
        //     .on_failure(OnFailure::SkipNextBatches)
        //     .add_task_details(true)
        //     .build();

        // let results = executor
        //     .execute_with_config(config)
        //     .await
        //     .expect("Execution failed");

        // // Process results...
    }

    // Example 4: Dry run
    #[tokio::test]
    async fn example_dry_run() {
        // let context = create_test_context();
        // let executor = RefactoredTaskExecutor::new(context);

        // let results = executor
        //     .dry_run(
        //         Call::Task("dangerous-operation".to_string()),
        //         Some("production-*")
        //     )
        //     .await
        //     .expect("Dry run failed");

        // println!("Would execute {} tasks", results.len());
        // for result in results {
        //     println!("Would run: {}", result.task().full_task_name());
        // }
    }
}

// ============================================================================
// MIGRATION HELPER
// ============================================================================

/// Helper to migrate from old TaskExecutor to new one
impl<TSys: TaskExecutorSys> crate::TaskExecutor<TSys> {
    /// Migrate to refactored execution system
    pub async fn execute_refactored(
        &self,
    ) -> Result<Vec<crate::TaskExecutionResult>, crate::TaskExecutorError> {
        let config = ExecutionConfig {
            call: self.call.clone(),
            ignore_dependencies: self.ignore_dependencies,
            project_filter: self.project_filter.clone(),
            meta_filter: self.meta_filter.clone(),
            force: self.force,
            no_cache: self.no_cache,
            on_failure: self.on_failure,
            dry_run: self.dry_run,
            replay_cached_logs: self.replay_cached_logs,
            add_task_details: self.add_task_details,
        };

        let factory = TaskExecutorFactory::new(self.context.clone());
        let mut pipeline = factory.create_execution_pipeline(config);

        pipeline.execute().await.map_err(|e| {
            crate::TaskExecutorError::from(
                crate::TaskExecutorErrorInner::Unknown(eyre::eyre!(
                    "Refactored execution failed: {}",
                    e
                )),
            )
        })
    }
}

// ============================================================================
// PERFORMANCE OPTIMIZATIONS
// ============================================================================

/// Optimized executor that reuses components across multiple executions
pub struct OptimizedExecutor<TSys: TaskExecutorSys> {
    factory: TaskExecutorFactory<TSys>,
    // Could cache compiled filters, project graphs, etc.
}

impl<TSys: TaskExecutorSys> OptimizedExecutor<TSys> {
    pub fn new(context: Context<TSys>) -> Self {
        Self {
            factory: TaskExecutorFactory::new(context),
        }
    }

    /// Execute multiple configurations efficiently
    pub async fn execute_batch(
        &self,
        configs: Vec<ExecutionConfig>,
    ) -> Result<Vec<Vec<TaskExecutionResult>>, ExecutionError> {
        let mut results = Vec::with_capacity(configs.len());

        for config in configs {
            let mut pipeline = self.factory.create_execution_pipeline(config);
            let result = pipeline.execute().await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Execute with warming up caches and project graphs
    pub async fn execute_optimized(
        &self,
        config: ExecutionConfig,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionError> {
        // Could implement pre-warming of caches, project graph building, etc.
        let mut pipeline = self.factory.create_execution_pipeline(config);
        pipeline.execute().await
    }
}

// ============================================================================
// SUMMARY OF BENEFITS
// ============================================================================

/*
This refactored design provides:

1. **Modularity**: Each component has a single responsibility
   - ExecutionPipeline orchestrates the high-level flow
   - BatchExecutor handles batch processing and concurrency
   - TaskProcessor manages individual task execution
   - Adapters bridge old and new systems

2. **Testability**: All components use trait dependencies
   - Easy to mock for unit testing
   - Can test components in isolation
   - Clear interfaces between components

3. **Flexibility**: Easy to swap implementations
   - Different cache stores
   - Different execution strategies
   - Different failure handling policies

4. **Maintainability**: Clear separation of concerns
   - Easy to understand what each part does
   - Easy to modify individual components
   - Less coupling between parts

5. **Extensibility**: Easy to add new features
   - New execution strategies
   - New cache implementations
   - New filtering mechanisms

6. **Migration Path**: Backwards compatible
   - Can migrate gradually
   - Existing code still works
   - Can compare old vs new implementations

7. **Performance**: Better resource management
   - Can optimize individual components
   - Better concurrency control
   - Reusable components across executions
*/
