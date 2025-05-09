use std::collections::{HashMap, HashSet};

use solana_sdk::pubkey::Pubkey;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::{cache::TaskStateRequest, sync};

pub type TaskStateSender = sync::MessageSender<TaskStateRequest>;
pub type TaskStateReceiver = sync::MessageReceiver<TaskStateRequest>;

impl TaskStateSender {
    pub async fn get_in_progress_tasks(&self, pubkey: Pubkey) -> Result<HashSet<u16>, sync::Error> {
        self.request(|resp| TaskStateRequest::GetInProgressTasks { pubkey, resp })
            .await
    }

    pub async fn add_in_progress_tasks(&self, pubkey: Pubkey, task_ids: HashSet<u16>) {
        self.send(TaskStateRequest::AddInProgressTasks { pubkey, task_ids })
            .await
    }

    pub async fn remove_in_progress_tasks(&self, pubkey: Pubkey, task_ids: HashSet<u16>) {
        self.send(TaskStateRequest::RemoveInProgressTasks { pubkey, task_ids })
            .await
    }
}

pub fn task_state_channel() -> (TaskStateSender, TaskStateReceiver) {
    let (tx, rx) = sync::message_channel(100);
    (tx, rx)
}

pub struct TaskStateCache {
    cache: HashMap<Pubkey, HashSet<u16>>,
    receiver: TaskStateReceiver,
}

impl TaskStateCache {
    pub fn new(receiver: TaskStateReceiver) -> Self {
        Self {
            cache: HashMap::new(),
            receiver,
        }
    }

    pub async fn run(mut self, handle: SubsystemHandle) -> anyhow::Result<()> {
        info!("starting task state cache");
        loop {
            tokio::select! {
                _ = handle.on_shutdown_requested() => {
                    info!("shutting down task state cache");
                    break;
                }
                Some(req) = self.receiver.recv() => {
                    match req {
                        TaskStateRequest::AddInProgressTasks { pubkey, task_ids } => {
                            self.cache.entry(pubkey).or_default().extend(task_ids);
                        }
                        TaskStateRequest::RemoveInProgressTasks { pubkey, task_ids } => {
                            if let Some(tasks) = self.cache.get_mut(&pubkey) {
                                for task_id in task_ids {
                                    tasks.remove(&task_id);
                                }
                                if tasks.is_empty() {
                                    self.cache.remove(&pubkey);
                                }
                            }
                        }
                        TaskStateRequest::GetInProgressTasks { pubkey, resp } => {
                            resp.send(self.cache.get(&pubkey).cloned().unwrap_or_default());
                        }
                    }
                }
                else => break,
            }
        }
        Ok(())
    }
}
