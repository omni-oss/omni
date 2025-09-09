pub struct TaskProcessor<'a, TSys: TaskExecutorSys> {
    executor: &'a TaskExecutor<TSys>,
    context: &'a Context<TSys>,
    cache_store: Option<&'a LocalTaskExecutionCacheStore>,
}

impl<'a, TSys: TaskExecutorSys> TaskProcessor<'a, TSys> {
    async fn process_task(
        &self,
        task_ctx: TaskContext<'_>,
    ) -> TaskProcessingResult {
        if !task_ctx.node.enabled() {
            return self.create_skipped_result(task_ctx, SkipReason::Disabled);
        }

        if self.should_skip_due_to_dependencies(&task_ctx)? {
            return self.create_dependency_skip_result(task_ctx);
        }

        if let Some(cached) = self.try_cache_hit(&task_ctx).await? {
            return self.handle_cache_hit(task_ctx, cached).await;
        }

        self.execute_task(task_ctx).await
    }
}
