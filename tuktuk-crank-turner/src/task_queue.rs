use std::{
    collections::{BinaryHeap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::Stream;
use solana_sdk::pubkey::Pubkey;
use tokio::{
    sync::{
        mpsc::{self, error::SendError, Sender},
        watch::Receiver,
        Mutex, Notify,
    },
    task::JoinHandle,
};

use crate::metrics::{DUPLICATE_TASKS, TASKS_IN_QUEUE, TASKS_NEXT_WAKEUP};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct TimedTask {
    pub task_time: u64,
    pub task_key: Pubkey,
    pub task_queue_key: Pubkey,
    pub task_queue_name: String,
    pub total_retries: u8,
    pub max_retries: u8,
    pub in_flight_task_ids: Vec<u16>,
}

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

pub struct TaskQueue {
    tx: Sender<TimedTask>,
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
}

pub struct TaskQueueArgs {
    pub channel_capacity: usize,
    pub now: Receiver<u64>,
}

pub struct TaskStream {
    task_queue: Arc<Mutex<BinaryHeap<TimedTask>>>,
    now: Receiver<u64>,
    seen_tasks: HashSet<u64>,
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
                    TASKS_IN_QUEUE
                        .with_label_values(&[task.task_queue_name.as_str()])
                        .dec();
                    let mut hasher = DefaultHasher::new();
                    task.hash(&mut hasher);
                    let hash = hasher.finish();
                    if this.seen_tasks.insert(hash) {
                        return Poll::Ready(Some(task));
                    } else {
                        DUPLICATE_TASKS
                            .with_label_values(&[task.task_queue_name.as_str()])
                            .inc();
                    }
                } else {
                    // Schedule a wake-up when the next task is ready
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

pub async fn create_task_queue(args: TaskQueueArgs) -> (TaskStream, TaskQueue) {
    let TaskQueueArgs {
        channel_capacity,
        now,
    } = args;
    let (tx, mut rx) = mpsc::channel::<TimedTask>(channel_capacity);
    let task_queue = Arc::new(Mutex::new(BinaryHeap::new()));

    let task_queue_clone = Arc::clone(&task_queue);

    let notify = Arc::new(Notify::new());
    let notify_clone = notify.clone();
    let message_thread = tokio::spawn(async move {
        while let Some(task) = rx.recv().await {
            let mut queue = task_queue_clone.lock().await;
            queue.push(task);
            notify_clone.notify_one(); // Notify when a new task is added
        }
    });

    let stream = TaskStream {
        task_queue: Arc::clone(&task_queue),
        now,
        seen_tasks: HashSet::new(),
        notify,
    };

    (stream, TaskQueue { tx, message_thread })
}
