use std::{borrow::Cow, collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::future::join_all;

// Re-export existing types that we'll use
use crate::{
    CacheInfo, Call, EnvVarsMap, OnFailure, SkipReason, TaskContext,
    TaskDetails, TaskExecutionNode, TaskExecutionResult,
};

// ============================================================================
// CORE TRAITS
// ============================================================================

#[async_trait]
pub trait ExecutionPipeline {
    type Error;
    async fn execute(
        &mut self,
    ) -> Result<Vec<TaskExecutionResult>, Self::Error>;
}

#[async_trait]
pub trait BatchExecutor {
    type Error;
    async fn execute_batch(
        &mut self,
        batch: &[TaskExecutionNode],
        previous_results: &HashMap<String, TaskExecutionResult>,
    ) -> Result<Vec<TaskExecutionResult>, Self::Error>;
}

#[async_trait]
pub trait TaskProcessor {
    type Error;
    async fn process_task(
        &self,
        task_ctx: TaskContext<'_>,
    ) -> Result<TaskExecutionResult, Self::Error>;
}

#[async_trait]
pub trait CacheStore {
    type Error;
    async fn get_cached_results(
        &self,
        inputs: &[TaskExecutionInfo<'_>],
    ) -> Result<HashMap<String, CachedTaskExecution>, Self::Error>;

    async fn cache_results(
        &self,
        results: &[CacheableResult<'_>],
    ) -> Result<HashMap<String, CachedTaskExecution>, Self::Error>;
}

pub trait ProjectProvider {
    type Error;
    fn get_filtered_projects(
        &self,
        filter: &str,
    ) -> Result<Vec<Project>, Self::Error>;
    fn get_execution_plan(
        &self,
        call: &Call,
        filter: &str,
        ignore_deps: bool,
    ) -> Result<Vec<Vec<TaskExecutionNode>>, Self::Error>;
    fn get_task_env_vars(
        &self,
        node: &TaskExecutionNode,
    ) -> Result<Cow<'_, EnvVarsMap>, Self::Error>;
    fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&CacheInfo>;
}

pub trait TaskFilter {
    type Error;
    fn should_include(
        &self,
        node: &TaskExecutionNode,
    ) -> Result<bool, Self::Error>;
}

#[async_trait]
pub trait ProcessRunner {
    type Error;
    async fn run_process(
        &self,
        node: &TaskExecutionNode,
        env_vars: &EnvVarsMap,
        record_logs: bool,
    ) -> Result<ChildProcessResult, Self::Error>;
}

// ============================================================================
// CONFIGURATION STRUCTS
// ============================================================================

#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    pub call: Call,
    pub ignore_dependencies: bool,
    pub project_filter: Option<String>,
    pub meta_filter: Option<String>,
    pub force: bool,
    pub no_cache: bool,
    pub on_failure: OnFailure,
    pub dry_run: bool,
    pub replay_cached_logs: bool,
    pub add_task_details: bool,
}

#[derive(Debug, Clone)]
pub struct BatchExecutionConfig {
    pub max_concurrency: usize,
    pub on_failure: OnFailure,
    pub replay_cached_logs: bool,
    pub dry_run: bool,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct TaskProcessingConfig {
    pub dry_run: bool,
    pub force: bool,
    pub no_cache: bool,
}

// ============================================================================
// PIPELINE IMPLEMENTATION
// ============================================================================

pub struct DefaultExecutionPipeline<P, C, B> {
    project_provider: P,
    cache_store: Option<C>,
    batch_executor: B,
    config: ExecutionConfig,
}

impl<P, C, B> DefaultExecutionPipeline<P, C, B> {
    pub fn new(
        project_provider: P,
        cache_store: Option<C>,
        batch_executor: B,
        config: ExecutionConfig,
    ) -> Self {
        Self {
            project_provider,
            cache_store,
            batch_executor,
            config,
        }
    }

    fn validate_input(&self) -> Result<(), ExecutionError> {
        if let Call::Task(task) = &self.config.call {
            if task.is_empty() {
                return Err(ExecutionError::TaskIsEmpty);
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<P, C, B> ExecutionPipeline for DefaultExecutionPipeline<P, C, B>
where
    P: ProjectProvider + Send + Sync,
    C: CacheStore + Send + Sync,
    B: BatchExecutor + Send + Sync,
    P::Error: Into<ExecutionError>,
    C::Error: Into<ExecutionError>,
    B::Error: Into<ExecutionError>,
{
    type Error = ExecutionError;

    async fn execute(
        &mut self,
    ) -> Result<Vec<TaskExecutionResult>, Self::Error> {
        let start = std::time::Instant::now();

        self.validate_input()?;

        let filter = self.config.project_filter.as_deref().unwrap_or("*");
        let execution_plan = self
            .project_provider
            .get_execution_plan(
                &self.config.call,
                filter,
                self.config.ignore_dependencies,
            )
            .map_err(Into::into)?;

        let task_count: usize = execution_plan.iter().map(|b| b.len()).sum();
        if task_count == 0 {
            return Err(ExecutionError::NothingToExecute(
                self.config.call.clone(),
            ));
        }

        let mut all_results =
            HashMap::<String, TaskExecutionResult>::with_capacity(task_count);

        for batch in execution_plan {
            let batch_results = self
                .batch_executor
                .execute_batch(&batch, &all_results)
                .await
                .map_err(Into::into)?;

            for result in batch_results {
                let key = result.task().full_task_name().to_string();
                all_results.insert(key, result);
            }
        }

        let mut results: Vec<_> = all_results.into_values().collect();

        if self.config.add_task_details {
            self.add_task_details(&mut results);
        }

        tracing::info!("Overall execution time: {:?}", start.elapsed());

        Ok(results)
    }
}

impl<P, C, B> DefaultExecutionPipeline<P, C, B>
where
    P: ProjectProvider,
{
    fn add_task_details(&self, results: &mut [TaskExecutionResult]) {
        for result in results {
            let task = result.task();
            let mut details = result.details().cloned().unwrap_or_default();

            if details.meta.is_none() {
                // This would need to be implemented based on your meta config logic
                // details.meta = self.project_provider.get_meta_config(task).cloned();
            }

            // Use the set_details method that should exist on TaskExecutionResult
            let mut result_mut = result.clone(); // This is a placeholder - you'd need mutable access
            // result_mut.set_details(details);
        }
    }
}

// ============================================================================
// BATCH EXECUTOR IMPLEMENTATION
// ============================================================================

pub struct ConcurrentBatchExecutor<T, C> {
    task_processor: T,
    cache_store: Option<C>,
    config: BatchExecutionConfig,
}

impl<T, C> ConcurrentBatchExecutor<T, C> {
    pub fn new(
        task_processor: T,
        cache_store: Option<C>,
        config: BatchExecutionConfig,
    ) -> Self {
        Self {
            task_processor,
            cache_store,
            config,
        }
    }

    fn should_skip_batch(
        &self,
        previous_results: &HashMap<String, TaskExecutionResult>,
    ) -> bool {
        self.config.on_failure == OnFailure::SkipNextBatches
            && previous_results.values().any(|r| r.is_error())
    }

    fn create_skipped_results(
        &self,
        batch: &[TaskExecutionNode],
    ) -> Vec<TaskExecutionResult> {
        batch
            .iter()
            .map(|task| {
                TaskExecutionResult::new_skipped(
                    task.clone(),
                    SkipReason::PreviousBatchFailure,
                )
            })
            .collect()
    }

    fn prepare_task_contexts<'a>(
        &self,
        batch: &'a [TaskExecutionNode],
        previous_results: &HashMap<String, TaskExecutionResult>,
    ) -> Vec<TaskContext<'a>> {
        batch
            .iter()
            .map(|node| {
                let dependencies =
                    if self.config.on_failure == OnFailure::SkipDependents {
                        node.dependencies()
                    } else {
                        &[]
                    };

                let dep_hashes = dependencies
                    .iter()
                    .filter_map(|d| {
                        previous_results.get(d).and_then(|r| r.hash())
                    })
                    .collect();

                TaskContext {
                    node,
                    dependencies,
                    dependency_hashes: dep_hashes,
                    env_vars: Cow::Borrowed(&EnvVarsMap::new()), // Placeholder
                    cache_info: None, // Would be filled by project provider
                }
            })
            .collect()
    }
}

#[async_trait]
impl<T, C> BatchExecutor for ConcurrentBatchExecutor<T, C>
where
    T: TaskProcessor + Clone + Send + Sync,
    C: CacheStore + Send + Sync,
    T::Error: Into<BatchExecutionError>,
    C::Error: Into<BatchExecutionError>,
{
    type Error = BatchExecutionError;

    async fn execute_batch(
        &mut self,
        batch: &[TaskExecutionNode],
        previous_results: &HashMap<String, TaskExecutionResult>,
    ) -> Result<Vec<TaskExecutionResult>, Self::Error> {
        if self.should_skip_batch(previous_results) {
            tracing::error!("Skipping batch due to previous failures");
            return Ok(self.create_skipped_results(batch));
        }

        let task_contexts = self.prepare_task_contexts(batch, previous_results);

        // Handle cache lookups if cache store is available
        let cached_results = if let Some(cache) = &self.cache_store {
            // This would need proper implementation with TaskExecutionInfo
            HashMap::new() // Placeholder
        } else {
            HashMap::new()
        };

        self.process_tasks_concurrently(task_contexts, cached_results)
            .await
    }
}

impl<T, C> ConcurrentBatchExecutor<T, C>
where
    T: TaskProcessor + Clone + Send + Sync,
    T::Error: Into<BatchExecutionError>,
{
    async fn process_tasks_concurrently(
        &self,
        task_contexts: Vec<TaskContext<'_>>,
        _cached_results: HashMap<String, CachedTaskExecution>,
    ) -> Result<Vec<TaskExecutionResult>, BatchExecutionError> {
        let mut futures = Vec::new();
        let mut results = Vec::new();

        for task_ctx in task_contexts {
            // Check for dependency failures
            if self.config.on_failure == OnFailure::SkipDependents
                && self.has_failed_dependencies(&task_ctx, &results)
            {
                results.push(TaskExecutionResult::new_skipped(
                    task_ctx.node.clone(),
                    SkipReason::DependeeTaskFailure,
                ));
                continue;
            }

            let processor = self.task_processor.clone();
            futures.push(async move {
                processor.process_task(task_ctx).await.map_err(Into::into)
            });

            // Limit concurrency
            if futures.len() >= self.config.max_concurrency {
                let batch_results = join_all(futures.drain(..)).await;
                for result in batch_results {
                    results.push(result?);
                }
            }
        }

        // Process remaining futures
        if !futures.is_empty() {
            let batch_results = join_all(futures).await;
            for result in batch_results {
                results.push(result?);
            }
        }

        Ok(results)
    }

    fn has_failed_dependencies(
        &self,
        task_ctx: &TaskContext<'_>,
        completed_results: &[TaskExecutionResult],
    ) -> bool {
        task_ctx.dependencies.iter().any(|dep| {
            completed_results.iter().any(|result| {
                result.task().full_task_name() == dep && result.is_failure()
            })
        })
    }
}

// ============================================================================
// TASK PROCESSOR IMPLEMENTATION
// ============================================================================

pub struct DefaultTaskProcessor<R, F> {
    process_runner: R,
    task_filter: F,
    config: TaskProcessingConfig,
}

impl<R, F> DefaultTaskProcessor<R, F> {
    pub fn new(
        process_runner: R,
        task_filter: F,
        config: TaskProcessingConfig,
    ) -> Self {
        Self {
            process_runner,
            task_filter,
            config,
        }
    }

    fn create_dry_run_result(
        &self,
        task_ctx: TaskContext<'_>,
    ) -> TaskExecutionResult {
        tracing::info!("Executing task '{}'", task_ctx.node.full_task_name());
        TaskExecutionResult::new_completed(
            None,
            task_ctx.node.clone(),
            0,
            Duration::ZERO,
            false,
        )
    }
}

#[async_trait]
impl<R, F> TaskProcessor for DefaultTaskProcessor<R, F>
where
    R: ProcessRunner + Send + Sync,
    F: TaskFilter + Send + Sync,
    R::Error: Into<TaskProcessingError>,
    F::Error: Into<TaskProcessingError>,
{
    type Error = TaskProcessingError;

    async fn process_task(
        &self,
        task_ctx: TaskContext<'_>,
    ) -> Result<TaskExecutionResult, Self::Error> {
        // Check if task should be included
        if !self
            .task_filter
            .should_include(task_ctx.node)
            .map_err(Into::into)?
        {
            return Ok(TaskExecutionResult::new_skipped(
                task_ctx.node.clone(),
                SkipReason::Disabled,
            ));
        }

        // Check if task is enabled
        if !task_ctx.node.enabled() {
            tracing::info!(
                "Skipping disabled task '{}'",
                task_ctx.node.full_task_name()
            );
            return Ok(TaskExecutionResult::new_skipped(
                task_ctx.node.clone(),
                SkipReason::Disabled,
            ));
        }

        // Handle dry run
        if self.config.dry_run {
            return Ok(self.create_dry_run_result(task_ctx));
        }

        // Execute the actual process
        let record_logs = task_ctx.cache_info.map_or(false, |ci| ci.cache_logs);
        let result = self
            .process_runner
            .run_process(task_ctx.node, &task_ctx.env_vars, record_logs)
            .await
            .map_err(Into::into)?;

        if result.success() {
            tracing::info!(
                "Executed task '{}'",
                task_ctx.node.full_task_name()
            );
        } else {
            tracing::error!(
                "Failed to execute task '{}', exit code '{}'",
                task_ctx.node.full_task_name(),
                result.exit_code()
            );
        }

        Ok(TaskExecutionResult::new_completed(
            None, // Hash will be added by cache layer if needed
            task_ctx.node.clone(),
            result.exit_code(),
            result.elapsed,
            false,
        ))
    }
}

impl<R, F> Clone for DefaultTaskProcessor<R, F>
where
    R: Clone,
    F: Clone,
{
    fn clone(&self) -> Self {
        Self {
            process_runner: self.process_runner.clone(),
            task_filter: self.task_filter.clone(),
            config: self.config.clone(),
        }
    }
}

// ============================================================================
// ERROR TYPES
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Task is empty")]
    TaskIsEmpty,

    #[error("No task to execute: {0} not found")]
    NothingToExecute(Call),

    #[error("Project provider error: {0}")]
    ProjectProvider(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Batch execution error: {0}")]
    BatchExecution(String),
}

#[derive(Debug, thiserror::Error)]
pub enum BatchExecutionError {
    #[error("Task processing error: {0}")]
    TaskProcessing(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Concurrency error: {0}")]
    Concurrency(String),
}

#[derive(Debug, thiserror::Error)]
pub enum TaskProcessingError {
    #[error("Process execution error: {0}")]
    ProcessExecution(String),

    #[error("Task filter error: {0}")]
    TaskFilter(String),

    #[error("Configuration error: {0}")]
    Configuration(String),
}

// ============================================================================
// PLACEHOLDER TYPES (to be replaced with actual implementations)
// ============================================================================

// These are placeholder types that would need to be implemented or imported
pub struct Project {
    pub name: String,
    pub dir: std::path::PathBuf,
    // ... other fields
}

pub struct TaskExecutionInfo<'a> {
    // ... fields for cache key generation
    _phantom: std::marker::PhantomData<&'a ()>,
}

pub struct CachedTaskExecution {
    // ... cached execution data
}

pub struct CacheableResult<'a> {
    // ... data to be cached
    _phantom: std::marker::PhantomData<&'a ()>,
}

pub struct ChildProcessResult {
    exit_code: u32,
    pub elapsed: Duration,
    // ... other fields
}

impl ChildProcessResult {
    pub fn new(
        _node: TaskExecutionNode,
        exit_code: u32,
        elapsed: Duration,
        _logs: Option<String>,
    ) -> Self {
        Self { exit_code, elapsed }
    }

    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn exit_code(&self) -> u32 {
        self.exit_code
    }
}
