use maps::UnorderedMap;
use tokio::task::JoinHandle;

pub type TasksMap<E> = UnorderedMap<String, JoinHandle<Result<(), E>>>;
