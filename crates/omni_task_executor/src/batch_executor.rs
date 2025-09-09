pub struct BatchExecutor<'a, TSys: TaskExecutorSys> {
    executor: &'a TaskExecutor<TSys>,
    context: &'a mut Context<TSys>,
    cache_store: Option<&'a LocalTaskExecutionCacheStore>,
    results_accumulator: &'a mut HashMap<String, TaskExecutionResult>,
}

impl<'a, TSys: TaskExecutorSys> BatchExecutor<'a, TSys> {
    async fn execute_batch(
        &mut self,
        batch: &[TaskExecutionNode],
    ) -> Result<(), TaskExecutorError> {
        if self.should_skip_batch()? {
            return self.skip_entire_batch(batch);
        }

        let task_contexts = self.prepare_task_contexts(batch)?;
        let cached_results = self.fetch_cached_results(&task_contexts).await?;

        self.process_tasks(task_contexts, cached_results).await
    }
}
