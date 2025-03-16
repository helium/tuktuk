use lazy_static::lazy_static;
use prometheus::{opts, IntCounterVec, IntGaugeVec, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref TASKS_IN_QUEUE: IntGaugeVec = IntGaugeVec::new(
        opts!("solana_tuktuk_tasks_in_queue", "Tasks in queue")
            .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
    pub static ref TASKS_NEXT_WAKEUP: IntGaugeVec = IntGaugeVec::new(
        opts!("solana_tuktuk_tasks_next_wakeup", "Tasks next wakeup")
            .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
    pub static ref TASKS_IN_PROGRESS: IntGaugeVec = IntGaugeVec::new(
        opts!("solana_tuktuk_tasks_in_progress", "Tasks in progress")
            .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
    pub static ref TASK_IDS_RESERVED: IntGaugeVec = IntGaugeVec::new(
        opts!("solana_tuktuk_task_ids_reserved", "Task ids reserved")
            .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
    pub static ref TASKS_COMPLETED: IntCounterVec = IntCounterVec::new(
        opts!("solana_tuktuk_tasks_completed", "Tasks completed")
            .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
    pub static ref TASKS_FAILED: IntCounterVec = IntCounterVec::new(
        opts!("solana_tuktuk_tasks_failed", "Tasks failed")
            .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue", "error_type"]
    )
    .expect("metric can be created");
    pub static ref TASK_QUEUE_PROFIT: IntGaugeVec = IntGaugeVec::new(
        opts!("solana_tuktuk_task_queue_profit", "Task queue profit")
            .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
    pub static ref TASK_ROLLING_PROFIT_PER_TASK: IntGaugeVec = IntGaugeVec::new(
        opts!(
            "solana_tuktuk_task_rolling_profit_per_task",
            "Task rolling profit per task"
        )
        .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
    pub static ref UPDATE_LAG: IntGaugeVec = IntGaugeVec::new(
        opts!("solana_tuktuk_update_lag", "Update lag")
            .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
    pub static ref UPDATES_VIA_POLL: IntCounterVec = IntCounterVec::new(
        opts!("solana_tuktuk_updates_via_poll", "Updates via poll")
            .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
    pub static ref UPDATES_VIA_WEBSOCKET: IntCounterVec = IntCounterVec::new(
        opts!(
            "solana_tuktuk_updates_via_websocket",
            "Updates via websocket"
        )
        .const_label("version", env!("CARGO_PKG_VERSION")),
        &["task_queue"]
    )
    .expect("metric can be created");
}

pub fn register_custom_metrics() {
    REGISTRY
        .register(Box::new(TASKS_IN_QUEUE.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(TASKS_IN_PROGRESS.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(TASKS_COMPLETED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(TASKS_FAILED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(TASKS_NEXT_WAKEUP.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(TASK_IDS_RESERVED.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(TASK_QUEUE_PROFIT.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(TASK_ROLLING_PROFIT_PER_TASK.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(UPDATE_LAG.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(UPDATES_VIA_POLL.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(UPDATES_VIA_WEBSOCKET.clone()))
        .expect("collector can be registered");
}
