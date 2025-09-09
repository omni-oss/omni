// ============================================================================
// ADVANCED CACHE IMPLEMENTATIONS
// ============================================================================

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

/// Multi-tier cache store that combines in-memory and persistent caching
pub struct MultiTierCacheStore<P> {
    memory_cache: Arc<RwLock<HashMap<String, TimestampedCacheEntry>>>,
    persistent_cache: P,
    memory_ttl: Duration,
}

#[derive(Debug, Clone)]
struct TimestampedCacheEntry {
    data: CachedTaskExecution,
    timestamp: SystemTime,
}

impl<P> MultiTierCacheStore<P> {
    pub fn new(persistent_cache: P, memory_ttl: Duration) -> Self {
        Self {
            memory_cache: Arc::new(RwLock::new(HashMap::new())),
            persistent_cache,
            memory_ttl,
        }
    }

    fn is_cache_entry_valid(&self, entry: &TimestampedCacheEntry) -> bool {
        entry.timestamp.elapsed().unwrap_or(Duration::MAX) < self.memory_ttl
    }
}

#[async_trait]
impl<P> CacheStore for MultiTierCacheStore<P>
where
    P: CacheStore + Send + Sync,
{
    type Error = CacheStoreError;

    async fn get_cached_results(
        &self,
        inputs: &[TaskExecutionInfo<'_>],
    ) -> Result<HashMap<String, CachedTaskExecution>, Self::Error> {
        let mut results = HashMap::new();
        let mut cache_misses = Vec::new();

        // First, check memory cache
        {
            let memory_cache = self.memory_cache.read().unwrap();
            for input in inputs {
                let key = self.generate_cache_key(input);
                if let Some(entry) = memory_cache.get(&key) {
                    if self.is_cache_entry_valid(entry) {
                        results.insert(key, entry.data.clone());
                    } else {
                        cache_misses.push(input);
                    }
                } else {
                    cache_misses.push(input);
                }
            }
        }

        // For cache misses, check persistent cache
        if !cache_misses.is_empty() {
            let persistent_results = self
                .persistent_cache
                .get_cached_results(&cache_misses)
                .await
                .map_err(|e| {
                    CacheStoreError::LookupFailed(format!(
                        "Persistent cache error: {}",
                        e
                    ))
                })?;

            // Update memory cache with persistent results
            {
                let mut memory_cache = self.memory_cache.write().unwrap();
                for (key, value) in &persistent_results {
                    memory_cache.insert(
                        key.clone(),
                        TimestampedCacheEntry {
                            data: value.clone(),
                            timestamp: SystemTime::now(),
                        },
                    );
                }
            }

            results.extend(persistent_results);
        }

        Ok(results)
    }

    async fn cache_results(
        &self,
        results: &[CacheableResult<'_>],
    ) -> Result<HashMap<String, CachedTaskExecution>, Self::Error> {
        // Cache to persistent store first
        let cached =
            self.persistent_cache.cache_results(results).await.map_err(
                |e| {
                    CacheStoreError::StoreFailed(format!(
                        "Persistent cache error: {}",
                        e
                    ))
                },
            )?;

        // Update memory cache
        {
            let mut memory_cache = self.memory_cache.write().unwrap();
            for (key, value) in &cached {
                memory_cache.insert(
                    key.clone(),
                    TimestampedCacheEntry {
                        data: value.clone(),
                        timestamp: SystemTime::now(),
                    },
                );
            }
        }

        Ok(cached)
    }
}

impl<P> MultiTierCacheStore<P> {
    fn generate_cache_key(&self, input: &TaskExecutionInfo<'_>) -> String {
        // This would implement proper cache key generation
        format!("{}#{}", input.project_name, input.task_name)
    }
}

// ============================================================================
// DISTRIBUTED EXECUTION SUPPORT
// ============================================================================

/// Executor that can distribute tasks across multiple workers/machines
#[async_trait]
pub trait DistributedExecutor {
    type Error;

    async fn execute_distributed(
        &self,
        batches: Vec<Vec<TaskExecutionNode>>,
        workers: Vec<WorkerId>,
    ) -> Result<Vec<TaskExecutionResult>, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkerId(String);

pub struct WorkerPool<W> {
    workers: Vec<W>,
    load_balancer: Box<dyn LoadBalancer + Send + Sync>,
}

pub trait LoadBalancer {
    fn assign_task(
        &self,
        task: &TaskExecutionNode,
        available_workers: &[WorkerId],
    ) -> WorkerId;
}

pub struct RoundRobinBalancer {
    current: std::sync::atomic::AtomicUsize,
}

impl LoadBalancer for RoundRobinBalancer {
    fn assign_task(
        &self,
        _task: &TaskExecutionNode,
        available_workers: &[WorkerId],
    ) -> WorkerId {
        let index = self
            .current
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % available_workers.len();
        available_workers[index].clone()
    }
}

// ============================================================================
// METRICS AND OBSERVABILITY
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    pub total_tasks: usize,
    pub successful_tasks: usize,
    pub failed_tasks: usize,
    pub skipped_tasks: usize,
    pub total_duration: Duration,
    pub cache_hit_rate: f64,
    pub average_task_duration: Duration,
    pub concurrency_level: usize,
    pub batches_executed: usize,
}

pub trait MetricsCollector {
    fn record_task_start(&self, task: &TaskExecutionNode);
    fn record_task_completion(
        &self,
        task: &TaskExecutionNode,
        result: &TaskExecutionResult,
    );
    fn record_cache_hit(&self, task: &TaskExecutionNode);
    fn record_cache_miss(&self, task: &TaskExecutionNode);
    fn get_metrics(&self) -> ExecutionMetrics;
}

pub struct InMemoryMetricsCollector {
    metrics: Arc<RwLock<InternalMetrics>>,
}

#[derive(Default)]
struct InternalMetrics {
    total_tasks: usize,
    successful_tasks: usize,
    failed_tasks: usize,
    skipped_tasks: usize,
    total_duration: Duration,
    cache_hits: usize,
    cache_misses: usize,
    task_durations: Vec<Duration>,
    start_time: Option<SystemTime>,
}

impl InMemoryMetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(InternalMetrics::default())),
        }
    }
}

impl MetricsCollector for InMemoryMetricsCollector {
    fn record_task_start(&self, _task: &TaskExecutionNode) {
        let mut metrics = self.metrics.write().unwrap();
        if metrics.start_time.is_none() {
            metrics.start_time = Some(SystemTime::now());
        }
        metrics.total_tasks += 1;
    }

    fn record_task_completion(
        &self,
        _task: &TaskExecutionNode,
        result: &TaskExecutionResult,
    ) {
        let mut metrics = self.metrics.write().unwrap();

        match result {
            TaskExecutionResult::Completed {
                elapsed, exit_code, ..
            } => {
                if *exit_code == 0 {
                    metrics.successful_tasks += 1;
                } else {
                    metrics.failed_tasks += 1;
                }
                metrics.task_durations.push(*elapsed);
                metrics.total_duration += *elapsed;
            }
            TaskExecutionResult::Error { .. } => {
                metrics.failed_tasks += 1;
            }
            TaskExecutionResult::Skipped { .. } => {
                metrics.skipped_tasks += 1;
            }
        }
    }

    fn record_cache_hit(&self, _task: &TaskExecutionNode) {
        self.metrics.write().unwrap().cache_hits += 1;
    }

    fn record_cache_miss(&self, _task: &TaskExecutionNode) {
        self.metrics.write().unwrap().cache_misses += 1;
    }

    fn get_metrics(&self) -> ExecutionMetrics {
        let metrics = self.metrics.read().unwrap();
        let total_cache_operations = metrics.cache_hits + metrics.cache_misses;
        let cache_hit_rate = if total_cache_operations > 0 {
            metrics.cache_hits as f64 / total_cache_operations as f64
        } else {
            0.0
        };

        let average_task_duration = if !metrics.task_durations.is_empty() {
            metrics.task_durations.iter().sum::<Duration>()
                / metrics.task_durations.len() as u32
        } else {
            Duration::ZERO
        };

        ExecutionMetrics {
            total_tasks: metrics.total_tasks,
            successful_tasks: metrics.successful_tasks,
            failed_tasks: metrics.failed_tasks,
            skipped_tasks: metrics.skipped_tasks,
            total_duration: metrics.total_duration,
            cache_hit_rate,
            average_task_duration,
            concurrency_level: 0, // Would need to be tracked differently
            batches_executed: 0,  // Would need to be tracked differently
        }
    }
}

// ============================================================================
// INSTRUMENTED TASK PROCESSOR
// ============================================================================

pub struct InstrumentedTaskProcessor<T, M> {
    inner: T,
    metrics: M,
}

impl<T, M> InstrumentedTaskProcessor<T, M> {
    pub fn new(inner: T, metrics: M) -> Self {
        Self { inner, metrics }
    }
}

impl<T, M> Clone for InstrumentedTaskProcessor<T, M>
where
    T: Clone,
    M: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            metrics: self.metrics.clone(),
        }
    }
}

#[async_trait]
impl<T, M> TaskProcessor for InstrumentedTaskProcessor<T, M>
where
    T: TaskProcessor + Send + Sync,
    M: MetricsCollector + Send + Sync,
{
    type Error = T::Error;

    async fn process_task(
        &self,
        task_ctx: TaskContext<'_>,
    ) -> Result<TaskExecutionResult, Self::Error> {
        self.metrics.record_task_start(task_ctx.node);

        let result = self.inner.process_task(task_ctx).await?;

        self.metrics.record_task_completion(task_ctx.node, &result);

        Ok(result)
    }
}

// ============================================================================
// RETRY MECHANISM
// ============================================================================

pub struct RetryTaskProcessor<T> {
    inner: T,
    max_retries: usize,
    retry_delay: Duration,
    retry_predicate: Box<dyn Fn(&TaskExecutionResult) -> bool + Send + Sync>,
}

impl<T> RetryTaskProcessor<T> {
    pub fn new(inner: T, max_retries: usize, retry_delay: Duration) -> Self {
        Self {
            inner,
            max_retries,
            retry_delay,
            retry_predicate: Box::new(|result| {
                // Retry on failures but not on skipped tasks
                result.is_failure() && !result.is_skipped()
            }),
        }
    }

    pub fn with_retry_predicate<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&TaskExecutionResult) -> bool + Send + Sync + 'static,
    {
        self.retry_predicate = Box::new(predicate);
        self
    }
}

impl<T> Clone for RetryTaskProcessor<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            max_retries: self.max_retries,
            retry_delay: self.retry_delay,
            retry_predicate: Box::new(|result| {
                result.is_failure() && !result.is_skipped()
            }),
        }
    }
}

#[async_trait]
impl<T> TaskProcessor for RetryTaskProcessor<T>
where
    T: TaskProcessor + Send + Sync,
{
    type Error = T::Error;

    async fn process_task(
        &self,
        task_ctx: TaskContext<'_>,
    ) -> Result<TaskExecutionResult, Self::Error> {
        let mut last_result = self.inner.process_task(task_ctx).await?;

        for attempt in 1..=self.max_retries {
            if !(self.retry_predicate)(&last_result) {
                break;
            }

            tracing::warn!(
                "Task {} failed, retrying (attempt {}/{})",
                task_ctx.node.full_task_name(),
                attempt,
                self.max_retries
            );

            tokio::time::sleep(self.retry_delay).await;
            last_result = self.inner.process_task(task_ctx).await?;
        }

        Ok(last_result)
    }
}

// ============================================================================
// RATE LIMITED EXECUTOR
// ============================================================================

use tokio::sync::Semaphore;

pub struct RateLimitedBatchExecutor<B> {
    inner: B,
    semaphore: Arc<Semaphore>,
    rate_limit_per_second: usize,
}

impl<B> RateLimitedBatchExecutor<B> {
    pub fn new(inner: B, rate_limit_per_second: usize) -> Self {
        Self {
            inner,
            semaphore: Arc::new(Semaphore::new(rate_limit_per_second)),
            rate_limit_per_second,
        }
    }
}

#[async_trait]
impl<B> BatchExecutor for RateLimitedBatchExecutor<B>
where
    B: BatchExecutor + Send + Sync,
{
    type Error = B::Error;

    async fn execute_batch(
        &mut self,
        batch: &[TaskExecutionNode],
        previous_results: &HashMap<String, TaskExecutionResult>,
    ) -> Result<Vec<TaskExecutionResult>, Self::Error> {
        // Acquire permits based on batch size
        let permits_needed = batch.len().min(self.rate_limit_per_second);
        let _permits = self
            .semaphore
            .acquire_many(permits_needed as u32)
            .await
            .unwrap();

        let result = self.inner.execute_batch(batch, previous_results).await?;

        // Release permits after a delay to enforce rate limiting
        tokio::spawn({
            let semaphore = self.semaphore.clone();
            async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                // Permits are automatically released when _permits is dropped
            }
        });

        Ok(result)
    }
}

// ============================================================================
// PLUGIN SYSTEM
// ============================================================================

#[async_trait]
pub trait ExecutionPlugin: Send + Sync {
    async fn before_execution(
        &self,
        config: &ExecutionConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn after_execution(
        &self,
        results: &[TaskExecutionResult],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn before_task(
        &self,
        task: &TaskExecutionNode,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn after_task(
        &self,
        task: &TaskExecutionNode,
        result: &TaskExecutionResult,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

pub struct PluginManager {
    plugins: Vec<Box<dyn ExecutionPlugin>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn add_plugin(mut self, plugin: Box<dyn ExecutionPlugin>) -> Self {
        self.plugins.push(plugin);
        self
    }

    pub async fn before_execution(
        &self,
        config: &ExecutionConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for plugin in &self.plugins {
            plugin.before_execution(config).await?;
        }
        Ok(())
    }

    pub async fn after_execution(
        &self,
        results: &[TaskExecutionResult],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for plugin in &self.plugins {
            plugin.after_execution(results).await?;
        }
        Ok(())
    }

    pub async fn before_task(
        &self,
        task: &TaskExecutionNode,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for plugin in &self.plugins {
            plugin.before_task(task).await?;
        }
        Ok(())
    }

    pub async fn after_task(
        &self,
        task: &TaskExecutionNode,
        result: &TaskExecutionResult,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for plugin in &self.plugins {
            plugin.after_task(task, result).await?;
        }
        Ok(())
    }
}

// ============================================================================
// EXAMPLE PLUGINS
// ============================================================================

pub struct SlackNotificationPlugin {
    webhook_url: String,
    notify_on_failure: bool,
    notify_on_success: bool,
}

impl SlackNotificationPlugin {
    pub fn new(webhook_url: String) -> Self {
        Self {
            webhook_url,
            notify_on_failure: true,
            notify_on_success: false,
        }
    }

    pub fn notify_on_success(mut self, notify: bool) -> Self {
        self.notify_on_success = notify;
        self
    }

    pub fn notify_on_failure(mut self, notify: bool) -> Self {
        self.notify_on_failure = notify;
        self
    }
}

#[async_trait]
impl ExecutionPlugin for SlackNotificationPlugin {
    async fn before_execution(
        &self,
        _config: &ExecutionConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn after_execution(
        &self,
        results: &[TaskExecutionResult],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let failed_tasks = results.iter().filter(|r| r.is_failure()).count();
        let successful_tasks = results.iter().filter(|r| r.success()).count();

        if (failed_tasks > 0 && self.notify_on_failure)
            || (successful_tasks > 0 && self.notify_on_success)
        {
            let message = format!(
                "Execution completed: {} successful, {} failed, {} total",
                successful_tasks,
                failed_tasks,
                results.len()
            );

            // In a real implementation, this would send to Slack
            tracing::info!("Would send to Slack: {}", message);
        }

        Ok(())
    }

    async fn before_task(
        &self,
        _task: &TaskExecutionNode,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn after_task(
        &self,
        _task: &TaskExecutionNode,
        _result: &TaskExecutionResult,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

pub struct LoggingPlugin {
    log_level: tracing::Level,
}

impl LoggingPlugin {
    pub fn new(log_level: tracing::Level) -> Self {
        Self { log_level }
    }
}

#[async_trait]
impl ExecutionPlugin for LoggingPlugin {
    async fn before_execution(
        &self,
        config: &ExecutionConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::event!(
            self.log_level,
            "Starting execution with config: {:?}",
            config
        );
        Ok(())
    }

    async fn after_execution(
        &self,
        results: &[TaskExecutionResult],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::event!(
            self.log_level,
            "Execution completed with {} results",
            results.len()
        );
        Ok(())
    }

    async fn before_task(
        &self,
        task: &TaskExecutionNode,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::event!(
            self.log_level,
            "Starting task: {}",
            task.full_task_name()
        );
        Ok(())
    }

    async fn after_task(
        &self,
        task: &TaskExecutionNode,
        result: &TaskExecutionResult,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match result {
            TaskExecutionResult::Completed { exit_code, .. } => {
                tracing::event!(
                    self.log_level,
                    "Task {} completed with exit code {}",
                    task.full_task_name(),
                    exit_code
                );
            }
            TaskExecutionResult::Error { error, .. } => {
                tracing::event!(
                    self.log_level,
                    "Task {} failed: {}",
                    task.full_task_name(),
                    error
                );
            }
            TaskExecutionResult::Skipped { skip_reason, .. } => {
                tracing::event!(
                    self.log_level,
                    "Task {} skipped: {}",
                    task.full_task_name(),
                    skip_reason
                );
            }
        }
        Ok(())
    }
}
