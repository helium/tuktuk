use std::collections::HashSet;

use solana_sdk::{address_lookup_table::AddressLookupTableAccount, pubkey::Pubkey};
use tuktuk_program::TaskQueueV0;

use crate::sync::ResponseSender;

// Request types for state management
#[derive(Debug)]
pub enum TaskStateRequest {
    AddInProgressTasks {
        pubkey: Pubkey,
        task_ids: HashSet<u16>,
    },
    RemoveInProgressTasks {
        pubkey: Pubkey,
        task_ids: HashSet<u16>,
    },
    GetInProgressTasks {
        pubkey: Pubkey,
        resp: ResponseSender<HashSet<u16>>,
    },
}

#[derive(Debug)]
pub enum LookupTableRequest {
    Get {
        lookup_table_keys: Vec<Pubkey>,
        resp: ResponseSender<Vec<AddressLookupTableAccount>>,
    },
}

#[derive(Debug)]
pub enum TaskQueueRequest {
    Get {
        pubkey: Pubkey,
        resp: ResponseSender<Option<TaskQueueV0>>,
    },
    Update {
        pubkey: Pubkey,
        queue: Box<TaskQueueV0>,
    },
}

mod lookup_tables;
mod task_queues;
mod task_state;

pub use lookup_tables::{lookup_tables_channel, LookupTablesCache, LookupTablesSender};
pub use task_queues::{task_queues_channel, TaskQueueCache, TaskQueuesSender};
pub use task_state::{task_state_channel, TaskStateCache, TaskStateSender};
