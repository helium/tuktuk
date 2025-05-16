use std::collections::HashMap;

use solana_sdk::pubkey::Pubkey;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;
use tuktuk_program::TaskQueueV0;

use crate::{cache::TaskQueueRequest, sync};

pub type TaskQueuesSender = sync::MessageSender<TaskQueueRequest>;
pub type TaskQueuesReceiver = sync::MessageReceiver<TaskQueueRequest>;

impl TaskQueuesSender {
    pub async fn get_task_queue(&self, pubkey: Pubkey) -> Result<Option<TaskQueueV0>, sync::Error> {
        self.request(|resp| TaskQueueRequest::Get { pubkey, resp })
            .await
    }

    pub async fn update_task_queue(&self, pubkey: Pubkey, queue: TaskQueueV0) {
        self.send(TaskQueueRequest::Update {
            pubkey,
            queue: Box::new(queue),
        })
        .await
    }
}

pub fn task_queues_channel() -> (TaskQueuesSender, TaskQueuesReceiver) {
    let (tx, rx) = sync::message_channel(100);
    (tx, rx)
}

pub struct TaskQueueCache {
    cache: HashMap<Pubkey, TaskQueueV0>,
    receiver: TaskQueuesReceiver,
}

impl TaskQueueCache {
    pub fn new(receiver: TaskQueuesReceiver) -> Self {
        Self {
            cache: HashMap::new(),
            receiver,
        }
    }

    pub async fn run(mut self, handle: SubsystemHandle) -> anyhow::Result<()> {
        info!("starting task queue cache");
        loop {
            tokio::select! {
                _ = handle.on_shutdown_requested() => {
                    info!("shutting down task queue cache");
                    break;
                }
                Some(req) = self.receiver.recv() => {
                    match req {
                        TaskQueueRequest::Get { pubkey, resp } => {
                            resp.send(self.cache.get(&pubkey).cloned());
                        }
                        TaskQueueRequest::Update { pubkey, queue } => {
                            self.cache.insert(pubkey, *queue);
                        }
                    }
                }
                else => break,
            }
        }
        Ok(())
    }
}
