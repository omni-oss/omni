// use std::{collections::HashMap, ffi::OsString, sync::Arc};

// use futures::io::AllowStdIo;
// use maps::{Map, UnorderedMap};
// use omni_cache::{CachedTaskExecution, impls::LocalTaskExecutionCacheStore};
// use omni_core::TaskExecutionNode;
// use omni_hasher::impls::DefaultHash;

// use crate::{context::Context, executor::TaskExecutor};

// pub async fn execute_task(
//     task: TaskExecutionNode,
//     ctx: Arc<Context>,
//     dependency_hashes: Option<Vec<DefaultHash>>,
//     execution_cache: Option<Arc<UnorderedMap<String, CachedTaskExecution>>>,
//     cache_store: Option<Arc<LocalTaskExecutionCacheStore>>,
// ) -> Result<TaskExecutionNode, (TaskExecutionNode, eyre::Report)> {
//     let result = {
//         let task = task.clone();

//         let mut exec = TaskExecutor::new(task);
//         exec.set_output_writer(AllowStdIo::new(std::io::stdout()));
//         if let Some(cache) = execution_cache {
//             exec.enable_skipping_execution(cache);
//         }

//         if let Some(store) = cache_store {
//             exec.enable_saving_caching(store);
//         }

//         if let Some(hashes) = dependency_hashes {
//             exec.set_dependency_hashes(hashes);
//         }

//         exec.exec().await
//     };
//     match result {
//         Ok(exec) => {
//             if exec.success() {
//                 Ok(task)
//             } else {
//                 let error =
//                     eyre::eyre!("exited with code {}", exec.exit_status);
//                 Err((task, error))
//             }
//         }
//         Err(e) => {
//             let error = eyre::eyre!(e);
//             Err((task, error))
//         }
//     }
// }

// fn vars_os(vars: &Map<String, String>) -> HashMap<OsString, OsString> {
//     vars.iter()
//         .map(|(k, v)| (k.into(), v.into()))
//         .collect::<HashMap<_, _>>()
// }
