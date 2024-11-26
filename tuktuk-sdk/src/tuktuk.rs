use std::{ops::Range, result::Result, sync::Arc};

use anchor_lang::{prelude::*, InstructionData};
use futures::{future::BoxFuture, Stream, StreamExt};
use itertools::Itertools;
use solana_sdk::{hash::hash, instruction::Instruction};
use tokio::sync::Mutex;

use crate::{error::Error, watcher::PubsubTracker};

declare_id!("tukpKuBbnQwG6yQbYRbeDM9Dk3D9fDkUpc6sytJsyGC");

declare_program!(tuktuk);

pub use self::tuktuk::{
    accounts::{TaskQueueNameMappingV0, TaskQueueV0, TaskV0, TuktukConfigV0},
    client, types,
};
use self::types::{InitializeTuktukConfigArgsV0, TriggerV0};

fn hash_name(name: &str) -> [u8; 32] {
    hash(name.as_bytes()).to_bytes()
}

pub fn config_key() -> Pubkey {
    Pubkey::find_program_address(&[b"tuktuk_config"], &ID).0
}

impl TriggerV0 {
    pub fn is_active(&self, now: i64) -> bool {
        match self {
            TriggerV0::Now => true,
            TriggerV0::Timestamp(ts) => now >= *ts,
        }
    }
}

pub fn task_queue_name_mapping_key(config_key: &Pubkey, name: &str) -> Pubkey {
    Pubkey::find_program_address(
        &[
            b"task_queue_name_mapping",
            config_key.as_ref(),
            &hash_name(name),
        ],
        &ID,
    )
    .0
}

#[derive(Debug)]
pub struct TaskQueueUpdate {
    pub task_queues: Vec<(Pubkey, Option<TaskQueueV0>)>,
    pub removed: Range<u32>,
}

pub fn create_config(
    payer: Pubkey,
    network_mint: Pubkey,
    authority: Option<Pubkey>,
    args: InitializeTuktukConfigArgsV0,
) -> Result<Instruction, Error> {
    let config_key = config_key();

    let create_ix = Instruction {
        program_id: ID,
        accounts: tuktuk::client::accounts::InitializeTuktukConfigV0 {
            payer,
            approver: payer,
            network_mint,
            authority: authority.unwrap_or(payer),
            tuktuk_config: config_key,
            system_program: solana_sdk::system_program::ID,
        }
        .to_account_metas(None),
        data: tuktuk::client::args::InitializeTuktukConfigV0 { args }.data(),
    };
    Ok(create_ix)
}

pub mod task_queue {
    use spl_associated_token_account::get_associated_token_address;
    use tuktuk::accounts::TuktukConfigV0;

    use self::tuktuk::types::InitializeTaskQueueArgsV0;
    use super::*;
    use crate::client::GetAnchorAccount;

    pub fn key(config_key: &Pubkey, next_task_queue_id: u32) -> Pubkey {
        Pubkey::find_program_address(
            &[
                b"task_queue",
                config_key.as_ref(),
                &next_task_queue_id.to_le_bytes()[..],
            ],
            &ID,
        )
        .0
    }

    pub fn keys(config_key: &Pubkey, config: &TuktukConfigV0) -> Result<Vec<Pubkey>, Error> {
        let queue_ids = 0..config.next_task_queue_id;
        let queue_keys = queue_ids.map(|id| self::key(config_key, id)).collect_vec();
        Ok(queue_keys)
    }

    pub async fn create<C: GetAnchorAccount>(
        client: &C,
        payer: Pubkey,
        args: InitializeTaskQueueArgsV0,
        update_authority: Option<Pubkey>,
        queue_authority: Option<Pubkey>,
    ) -> Result<(Pubkey, Instruction), Error> {
        let config_key = config_key();
        let config: TuktukConfigV0 = client
            .anchor_account(&config_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        let queue_key = self::key(&config_key, config.next_task_queue_id);

        Ok((
            queue_key,
            Instruction {
                program_id: ID,
                accounts: tuktuk::client::accounts::InitializeTaskQueueV0 {
                    task_queue: queue_key,
                    payer,
                    system_program: solana_sdk::system_program::ID,
                    tuktuk_config: config_key,
                    network_mint: config.network_mint,
                    update_authority: update_authority.unwrap_or(payer),
                    rewards_source: get_associated_token_address(&queue_key, &config.network_mint),
                    token_program: spl_token::id(),
                    associated_token_program: spl_associated_token_account::ID,
                    task_queue_name_mapping: task_queue_name_mapping_key(&config_key, &args.name),
                    queue_authority: queue_authority.unwrap_or(payer),
                }
                .to_account_metas(None),
                data: tuktuk::client::args::InitializeTaskQueueV0 { args }.data(),
            },
        ))
    }

    pub async fn close<C: GetAnchorAccount>(
        client: &C,
        task_queue_key: Pubkey,
        payer: Pubkey,
        refund: Pubkey,
    ) -> Result<Instruction, Error> {
        let config_key = config_key();
        let config: TuktukConfigV0 = client
            .anchor_account(&config_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        let queue: TaskQueueV0 = client
            .anchor_account(&task_queue_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        Ok(Instruction {
            program_id: ID,
            accounts: tuktuk::client::accounts::CloseTaskQueueV0 {
                task_queue: task_queue_key,
                refund,
                rewards_refund: get_associated_token_address(&refund, &config.network_mint),
                rewards_source: get_associated_token_address(&task_queue_key, &config.network_mint),
                task_queue_name_mapping: task_queue_name_mapping_key(&config_key, &queue.name),
                payer,
                system_program: solana_sdk::system_program::ID,
                tuktuk_config: config_key,
                network_mint: config.network_mint,
                associated_token_program: spl_associated_token_account::ID,
                token_program: spl_token::id(),
                update_authority: queue.update_authority,
            }
            .to_account_metas(None),
            data: tuktuk::client::args::CloseTaskQueueV0 {}.data(),
        })
    }

    pub async fn on_new<'a, C: GetAnchorAccount>(
        client: &'a C,
        pubsub_tracker: &'a PubsubTracker,
        config_key: &'a Pubkey,
        config: &'a TuktukConfigV0,
    ) -> Result<
        (
            impl Stream<Item = Result<TaskQueueUpdate, Error>> + 'a,
            Box<dyn FnOnce() -> BoxFuture<'a, ()> + 'a>,
        ),
        Error,
    > {
        let (stream, unsubscribe) = pubsub_tracker.watch_pubkey(*config_key).await?;

        let last_id = Arc::new(Mutex::new(config.next_task_queue_id));
        let min_id = Arc::new(Mutex::new(0));
        let result = stream.then(move |acc| {
            let last_id = Arc::clone(&last_id);
            let min_id = Arc::clone(&min_id);
            async move {
                let mut last_id_value = last_id.lock().await;
                let mut min_id_value = min_id.lock().await;
                let last_id = *last_id_value;
                let min_id = *min_id_value;

                let new_config = TuktukConfigV0::try_deserialize(&mut acc?.data.as_ref())?;
                *last_id_value = new_config.next_task_queue_id;
                *min_id_value = 0;
                let queue_ids = last_id..new_config.next_task_queue_id;
                let queue_keys = queue_ids
                    .clone()
                    .map(|id| self::key(config_key, id))
                    .collect::<Vec<_>>();

                let queues = client.anchor_accounts(&queue_keys).await?;
                Ok(TaskQueueUpdate {
                    task_queues: queues,
                    removed: min_id..last_id,
                })
            }
        });

        Ok((result, unsubscribe))
    }
}

#[derive(Debug)]
pub struct TaskUpdate {
    pub tasks: Vec<(Pubkey, Option<TaskV0>)>,
    pub removed: Vec<Pubkey>,
}

impl TaskQueueV0 {
    pub fn task_exists(&self, task_idx: usize) -> bool {
        self.task_bitmap[task_idx / 8] & (1 << (task_idx % 8)) != 0
    }

    pub fn next_available_task_id(&self) -> Option<usize> {
        for (byte_idx, byte) in self.task_bitmap.iter().enumerate() {
            if *byte != 0xff {
                // If byte is not all 1s
                for bit_idx in 0..8 {
                    if byte & (1 << bit_idx) == 0 {
                        return Some(byte_idx * 8 + bit_idx);
                    }
                }
            }
        }
        None
    }
}

pub mod task {
    use std::sync::Arc;

    use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
    use futures::{future::BoxFuture, Stream, StreamExt};
    use itertools::Itertools;
    use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
    use tokio::sync::Mutex;

    use super::{
        tuktuk::{self, accounts::TaskQueueV0},
        types::QueueTaskArgsV0,
        TaskUpdate, ID,
    };
    use crate::{client::GetAnchorAccount, error::Error, watcher::PubsubTracker};

    pub fn key(queue_key: &Pubkey, task_id: u16) -> Pubkey {
        Pubkey::find_program_address(
            &[
                "task".as_bytes(),
                queue_key.as_ref(),
                &task_id.to_le_bytes()[..],
            ],
            &ID,
        )
        .0
    }

    pub fn keys(queue_key: &Pubkey, task_queue: &TaskQueueV0) -> Result<Vec<Pubkey>, Error> {
        let task_ids = 0..task_queue.capacity;
        let task_keys = task_ids
            .filter(|k| task_queue.task_exists(*k as usize))
            .map(|id| self::key(queue_key, id))
            .collect_vec();
        Ok(task_keys)
    }

    pub fn queue_ix(
        task_queue_key: Pubkey,
        task_queue: &TaskQueueV0,
        payer: Pubkey,
        args: QueueTaskArgsV0,
    ) -> Result<(Pubkey, Instruction), Error> {
        let task_key = self::key(
            &task_queue_key,
            task_queue
                .next_available_task_id()
                .ok_or_else(|| Error::TooManyTasks)? as u16,
        );

        Ok((
            task_key,
            Instruction {
                program_id: ID,
                accounts: tuktuk::client::accounts::QueueTaskV0 {
                    task_queue: task_queue_key,
                    payer,
                    system_program: solana_sdk::system_program::ID,
                    task: task_key,
                    queue_authority: task_queue.queue_authority,
                }
                .to_account_metas(None),
                data: tuktuk::client::args::QueueTaskV0 { args }.data(),
            },
        ))
    }

    pub async fn queue<C: GetAnchorAccount>(
        client: &C,
        payer: Pubkey,
        task_queue_key: Pubkey,
        args: QueueTaskArgsV0,
    ) -> Result<(Pubkey, Instruction), Error> {
        let task_queue: TaskQueueV0 = client
            .anchor_account(&task_queue_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        self::queue_ix(task_queue_key, &task_queue, payer, args)
    }

    pub async fn on_new<'a, C: GetAnchorAccount>(
        client: &'a C,
        pubsub_tracker: &'a PubsubTracker,
        task_queue_key: &'a Pubkey,
        task_queue: &'a TaskQueueV0,
    ) -> Result<
        (
            impl Stream<Item = Result<TaskUpdate, Error>> + 'a,
            Box<dyn FnOnce() -> BoxFuture<'a, ()> + 'a>,
        ),
        Error,
    > {
        let (stream, unsubscribe) = pubsub_tracker.watch_pubkey(*task_queue_key).await?;

        let last_tq = Arc::new(Mutex::new(task_queue.clone()));
        let result = stream.then(move |acc| {
            let last_tq = last_tq.clone();
            async move {
                let mut last_tq_guard = last_tq.lock().await;
                let last_tq_clone = last_tq_guard.clone();

                let new_task_queue = TaskQueueV0::try_deserialize(&mut acc?.data.as_ref())?;
                *last_tq_guard = new_task_queue.clone();
                let task_ids = 0..new_task_queue.capacity;
                let new_task_keys = task_ids
                    .clone()
                    .filter(|id| {
                        new_task_queue.task_exists(*id as usize)
                            && !last_tq_clone.task_exists(*id as usize)
                    })
                    .map(|id| self::key(task_queue_key, id))
                    .collect_vec();

                let removed_task_keys = task_ids
                    .clone()
                    .filter(|id| {
                        !new_task_queue.task_exists(*id as usize)
                            && last_tq_clone.task_exists(*id as usize)
                    })
                    .map(|id| self::key(task_queue_key, id))
                    .collect_vec();

                let tasks = client.anchor_accounts(&new_task_keys).await?;
                Ok(TaskUpdate {
                    tasks,
                    removed: removed_task_keys,
                })
            }
        });

        Ok((result, unsubscribe))
    }
}
