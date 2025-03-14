use std::{
    collections::BinaryHeap,
    num::NonZero,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::Stream;
use lru::LruCache;
use solana_sdk::pubkey::Pubkey;
use tokio::{
    sync::{
        mpsc::{self, error::SendError, Sender},
        watch::Receiver,
        Mutex, Notify,
    },
    task::JoinHandle,
};
use tuktuk_program::TaskV0;
use tuktuk_sdk::compiled_transaction::RunTaskResult;

use crate::metrics::{TASKS_IN_QUEUE, TASKS_NEXT_WAKEUP};

#[derive(Debug, Clone)]
pub struct TimedTask {
    pub task_time: u64,
    pub task_key: Pubkey,
    pub task: TaskV0,
    pub task_queue_key: Pubkey,
    pub task_queue_name: String,
    pub total_retries: u8,
    pub max_retries: u8,
    pub in_flight_task_ids: Vec<u16>,
    pub is_cleanup_task: bool,
    pub profitability_delayed: bool,
    pub cached_result: Option<RunTaskResult>,
}

impl PartialEq for TimedTask {
    fn eq(&self, other: &Self) -> bool {
        self.task_key == other.task_key && self.task_time == other.task_time
    }
}

impl Eq for TimedTask {}

// Implement Ord and PartialOrd for priority queue
impl Ord for TimedTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.task_time.cmp(&self.task_time) // Max-heap
    }
}

impl PartialOrd for TimedTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Hash, Eq, PartialEq)]
pub struct RemovedTaskKey {
    task_key: Pubkey,
    queued_at: i64,
}

pub struct TaskQueue {
    tx: Sender<TimedTask>,
    removal_tx: Sender<Pubkey>,
    message_thread: JoinHandle<()>,
}

impl TaskQueue {
    pub async fn add_task(&self, task: TimedTask) -> Result<(), SendError<TimedTask>> {
        TASKS_IN_QUEUE
            .with_label_values(&[task.task_queue_name.as_str()])
            .inc();
        self.tx.send(task).await
    }
    pub async fn abort(&self) {
        self.message_thread.abort();
    }
    pub async fn remove_task(&self, task_key: Pubkey) -> Result<(), SendError<Pubkey>> {
        self.removal_tx.send(task_key).await
    }
}

pub struct TaskQueueArgs {
    pub channel_capacity: usize,
    pub now: Receiver<u64>,
}

pub struct TaskStream {
    task_queue: Arc<Mutex<BinaryHeap<TimedTask>>>,
    removed_tasks: Arc<Mutex<LruCache<RemovedTaskKey, ()>>>,
    now: Receiver<u64>,
    notify: Arc<Notify>,
}

impl Stream for TaskStream {
    type Item = TimedTask;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let now = *this.now.borrow();

        if let Ok(mut queue) = this.task_queue.try_lock() {
            if let Some(task) = queue.peek() {
                if task.task_time <= now {
                    let task = queue.pop().unwrap();

                    // Check if task was removed
                    let removed_tasks = this.removed_tasks.try_lock().unwrap();
                    let key = RemovedTaskKey {
                        task_key: task.task_key,
                        queued_at: task.task.queued_at,
                    };

                    if removed_tasks.contains(&key) {
                        // Task was removed, continue polling
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }

                    TASKS_IN_QUEUE
                        .with_label_values(&[task.task_queue_name.as_str()])
                        .dec();
                    TASKS_NEXT_WAKEUP
                        .with_label_values(&[task.task_queue_name.as_str()])
                        .set(0);
                    return Poll::Ready(Some(task));
                } else {
                    let wake_time = task.task_time.saturating_sub(now);
                    TASKS_NEXT_WAKEUP
                        .with_label_values(&[task.task_queue_name.as_str()])
                        .set(task.task_time as i64);
                    let waker = cx.waker().clone();
                    let notify = this.notify.clone();
                    tokio::spawn(async move {
                        tokio::select! {
                            _ = tokio::time::sleep(std::time::Duration::from_secs(wake_time)) => {},
                            _ = notify.notified() => {},
                        }
                        waker.wake();
                    });
                }
            } else {
                // Schedule a wake-up after a reasonable delay when queue is empty
                let waker = cx.waker().clone();
                let notify = this.notify.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
                        _ = notify.notified() => {},
                    }
                    waker.wake();
                });
            }
        } else {
            cx.waker().wake_by_ref();
        }

        Poll::Pending
    }
}

const REMOVAL_LRU_SIZE: usize = 500;
const QUEUED_AT_LRU_SIZE: usize = 5000;
pub async fn create_task_queue(args: TaskQueueArgs) -> (TaskStream, TaskQueue) {
    let (tx, mut rx) = mpsc::channel::<TimedTask>(args.channel_capacity);
    let (removal_tx, mut removal_rx) = mpsc::channel::<Pubkey>(args.channel_capacity);
    let task_queue = Arc::new(Mutex::new(BinaryHeap::new()));
    let removed_tasks = Arc::new(Mutex::new(LruCache::new(NonZero::new(REMOVAL_LRU_SIZE).unwrap()))); // Keep last n removed tasks

    // Keep last n queued ats, since the watcher does not know the queue_at of removed task, just that the pubkey disappeared. And we don't want to permanently taint a pubkey,
    // we just want to taint that pubkey for the queue_at time of the task.
    let task_queued_ats = Arc::new(Mutex::new(LruCache::new(NonZero::new(QUEUED_AT_LRU_SIZE).unwrap())));
    let notify = Arc::new(Notify::new());

    // Handle both tasks and removals
    let message_thread = {
        let task_queue = task_queue.clone();
        let removed_tasks = removed_tasks.clone();
        let notify = notify.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(task) = rx.recv() => {
                        task_queued_ats.lock().await.put(task.task_key, task.task.queued_at);
                        task_queue.lock().await.push(task);
                        notify.notify_one();
                    }
                    Some(removed) = removal_rx.recv() => {
                        if let Some(queued_at) = task_queued_ats.lock().await.get(&removed) {
                            removed_tasks.lock().await.put(RemovedTaskKey {
                                task_key: removed,
                                queued_at: *queued_at,
                            }, ());
                        }
                    }
                    else => break,
                }
            }
        })
    };

    let stream = TaskStream {
        task_queue: Arc::clone(&task_queue),
        removed_tasks: removed_tasks.clone(),
        now: args.now,
        notify,
    };

    (
        stream,
        TaskQueue {
            tx,
            removal_tx,
            message_thread,
        },
    )
}
