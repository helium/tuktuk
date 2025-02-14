use std::{ops::Range, result::Result, sync::Arc};

use anchor_lang::{prelude::*, InstructionData};
use futures::{future::BoxFuture, Stream, StreamExt};
use itertools::Itertools;
use solana_sdk::{hash::hash, instruction::Instruction};
use tokio::sync::Mutex;
use tuktuk_program::*;

use crate::{error::Error, watcher::PubsubTracker};

fn hash_name(name: &str) -> [u8; 32] {
    hash(name.as_bytes()).to_bytes()
}

pub fn config_key() -> Pubkey {
    Pubkey::find_program_address(&[b"tuktuk_config"], &tuktuk::ID).0
}

pub fn task_queue_name_mapping_key(config_key: &Pubkey, name: &str) -> Pubkey {
    Pubkey::find_program_address(
        &[
            b"task_queue_name_mapping",
            config_key.as_ref(),
            &hash_name(name),
        ],
        &tuktuk::ID,
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
    authority: Option<Pubkey>,
    args: InitializeTuktukConfigArgsV0,
) -> Result<Instruction, Error> {
    let config_key = config_key();

    let create_ix = Instruction {
        program_id: tuktuk::ID,
        accounts: tuktuk::client::accounts::InitializeTuktukConfigV0 {
            payer,
            approver: payer,
            authority: authority.unwrap_or(payer),
            tuktuk_config: config_key,
            system_program: solana_sdk::system_program::ID,
        }
        .to_account_metas(None),
        data: tuktuk::client::args::InitializeTuktukConfigV0 { args }.data(),
    };
    Ok(create_ix)
}

pub mod cron {
    use anchor_lang::{InstructionData, ToAccountMetas};
    use itertools::Itertools;
    use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
    use tuktuk_program::{
        cron::{
            self,
            cron::{
                accounts::{CronJobV0, UserCronJobsV0},
                types::InitializeCronJobArgsV0,
                ID,
            },
        },
        TaskQueueV0,
    };

    use super::{hash_name, task, task_queue::task_queue_authority_key};
    use crate::{client::GetAnchorAccount, error::Error};

    pub fn user_cron_jobs_key(authority: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"user_cron_jobs", authority.as_ref()], &ID).0
    }

    pub fn cron_job_key(authority: &Pubkey, cron_job_id: u32) -> Pubkey {
        Pubkey::find_program_address(
            &[
                b"cron_job",
                authority.as_ref(),
                &cron_job_id.to_le_bytes()[..],
            ],
            &cron::cron::ID,
        )
        .0
    }

    pub fn name_mapping_key(authority: &Pubkey, name: &str) -> Pubkey {
        Pubkey::find_program_address(
            &[
                b"cron_job_name_mapping",
                authority.as_ref(),
                &hash_name(name),
            ],
            &cron::cron::ID,
        )
        .0
    }

    pub fn task_return_account_1_key(cron_job: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[b"task_return_account_1", cron_job.as_ref()],
            &cron::cron::ID,
        )
        .0
    }

    pub fn task_return_account_2_key(cron_job: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[b"task_return_account_2", cron_job.as_ref()],
            &cron::cron::ID,
        )
        .0
    }

    pub fn keys(authority: &Pubkey, user_cron_jobs: &UserCronJobsV0) -> Result<Vec<Pubkey>, Error> {
        let cron_job_ids = 0..user_cron_jobs.next_cron_job_id;
        let cron_job_keys = cron_job_ids
            .map(|id| self::cron_job_key(authority, id))
            .collect_vec();
        Ok(cron_job_keys)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_ix(
        payer: Pubkey,
        authority: Pubkey,
        user_crons_key: Pubkey,
        cron_job_key: Pubkey,
        task_queue_key: Pubkey,
        queue_authority: Pubkey,
        task_id: u16,
        args: InitializeCronJobArgsV0,
    ) -> Result<Instruction, Error> {
        Ok(Instruction {
            program_id: ID,
            accounts: cron::cron::client::accounts::InitializeCronJobV0 {
                task_queue: task_queue_key,
                payer,
                system_program: solana_sdk::system_program::ID,
                authority,
                user_cron_jobs: user_crons_key,
                cron_job: cron_job_key,
                cron_job_name_mapping: self::name_mapping_key(&authority, &args.name),
                task: task::key(&task_queue_key, task_id),
                tuktuk_program: tuktuk_program::tuktuk::ID,
                queue_authority,
                task_return_account_1: self::task_return_account_1_key(&cron_job_key),
                task_return_account_2: self::task_return_account_2_key(&cron_job_key),
                task_queue_authority: task_queue_authority_key(&task_queue_key, &queue_authority),
            }
            .to_account_metas(None),
            data: cron::cron::client::args::InitializeCronJobV0 { args }.data(),
        })
    }

    pub async fn create<C: GetAnchorAccount>(
        client: &C,
        payer: Pubkey,
        queue_authority: Pubkey,
        args: InitializeCronJobArgsV0,
        authority: Option<Pubkey>,
        task_queue_key: Pubkey,
    ) -> Result<(Pubkey, Instruction), Error> {
        let authority = authority.unwrap_or(payer);
        let user_crons_key = self::user_cron_jobs_key(&authority);
        let user_cron_jobs: Option<UserCronJobsV0> = client.anchor_account(&user_crons_key).await?;

        let cron_job_key = self::cron_job_key(
            &authority,
            user_cron_jobs.map_or(0, |ucj| ucj.next_cron_job_id),
        );
        let task_queue: TaskQueueV0 = client
            .anchor_account(&task_queue_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        let ix = create_ix(
            payer,
            authority,
            user_crons_key,
            cron_job_key,
            task_queue_key,
            queue_authority,
            task_queue.next_available_task_id().unwrap(),
            args,
        )?;

        Ok((cron_job_key, ix))
    }

    pub fn close_ix(
        cron_job_key: Pubkey,
        payer: Pubkey,
        authority: Pubkey,
        rent_refund: Pubkey,
        user_crons_key: Pubkey,
        name: String,
    ) -> Result<Instruction, Error> {
        Ok(Instruction {
            program_id: cron::cron::ID,
            accounts: cron::cron::client::accounts::CloseCronJobV0 {
                rent_refund,
                payer,
                authority: payer,
                user_cron_jobs: user_crons_key,
                cron_job: cron_job_key,
                cron_job_name_mapping: self::name_mapping_key(&authority, &name),
                system_program: solana_sdk::system_program::ID,
            }
            .to_account_metas(None),
            data: cron::cron::client::args::CloseCronJobV0 {}.data(),
        })
    }

    pub async fn close<C: GetAnchorAccount>(
        client: &C,
        cron_job_key: Pubkey,
        payer: Pubkey,
        authority: Option<Pubkey>,
        rent_refund: Option<Pubkey>,
    ) -> Result<Instruction, Error> {
        let authority = authority.unwrap_or(payer);
        let rent_refund = rent_refund.unwrap_or(payer);
        let user_crons_key = self::user_cron_jobs_key(&authority);
        let cron_job: CronJobV0 = client
            .anchor_account(&cron_job_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        close_ix(
            cron_job_key,
            payer,
            authority,
            rent_refund,
            user_crons_key,
            cron_job.name,
        )
    }
}

pub mod cron_job_transaction {
    use anchor_lang::{InstructionData, ToAccountMetas};
    use itertools::Itertools;
    use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
    use tuktuk_program::cron::{
        self,
        cron::{
            accounts::CronJobV0,
            types::{AddCronTransactionArgsV0, RemoveCronTransactionArgsV0},
        },
    };

    use crate::error::Error;

    pub fn key(cron_job_key: &Pubkey, cron_job_transaction_id: u32) -> Pubkey {
        Pubkey::find_program_address(
            &[
                b"cron_job_transaction",
                cron_job_key.as_ref(),
                &cron_job_transaction_id.to_le_bytes()[..],
            ],
            &cron::cron::ID,
        )
        .0
    }

    pub fn keys(cron_job_key: &Pubkey, cron_job: &CronJobV0) -> Result<Vec<Pubkey>, Error> {
        let cron_job_transaction_ids = 0..cron_job.next_transaction_id;
        let cron_job_transaction_keys = cron_job_transaction_ids
            .map(|id| self::key(cron_job_key, id))
            .collect_vec();
        Ok(cron_job_transaction_keys)
    }

    pub fn add_transaction(
        payer: Pubkey,
        cron_job_key: Pubkey,
        args: AddCronTransactionArgsV0,
    ) -> Result<(Pubkey, Instruction), Error> {
        let cron_job_transaction_key = self::key(&cron_job_key, args.index);

        Ok((
            cron_job_transaction_key,
            Instruction {
                program_id: cron::cron::ID,
                accounts: cron::cron::client::accounts::AddCronTransactionV0 {
                    payer,
                    cron_job: cron_job_key,
                    cron_job_transaction: cron_job_transaction_key,
                    system_program: solana_sdk::system_program::ID,
                    authority: payer,
                }
                .to_account_metas(None),
                data: cron::cron::client::args::AddCronTransactionV0 { args }.data(),
            },
        ))
    }

    pub fn remove_transaction(
        payer: Pubkey,
        cron_job_key: Pubkey,
        args: RemoveCronTransactionArgsV0,
    ) -> Result<Instruction, Error> {
        let cron_job_transaction_key = self::key(&cron_job_key, args.index);

        Ok(Instruction {
            program_id: cron::cron::ID,
            accounts: cron::cron::client::accounts::RemoveCronTransactionV0 {
                rent_refund: payer,
                authority: payer,
                cron_job: cron_job_key,
                cron_job_transaction: cron_job_transaction_key,
                system_program: solana_sdk::system_program::ID,
            }
            .to_account_metas(None),
            data: cron::cron::client::args::RemoveCronTransactionV0 { args }.data(),
        })
    }
}

pub mod task_queue {
    use tuktuk::accounts::TuktukConfigV0;
    use tuktuk_program::types::UpdateTaskQueueArgsV0;

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
            &tuktuk::ID,
        )
        .0
    }

    pub fn task_queue_authority_key(task_queue_key: &Pubkey, queue_authority: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[
                b"task_queue_authority",
                task_queue_key.as_ref(),
                queue_authority.as_ref(),
            ],
            &tuktuk::ID,
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
                program_id: tuktuk::ID,
                accounts: tuktuk::client::accounts::InitializeTaskQueueV0 {
                    task_queue: queue_key,
                    payer,
                    system_program: solana_sdk::system_program::ID,
                    tuktuk_config: config_key,
                    update_authority: update_authority.unwrap_or(payer),
                    task_queue_name_mapping: task_queue_name_mapping_key(&config_key, &args.name),
                }
                .to_account_metas(None),
                data: tuktuk::client::args::InitializeTaskQueueV0 { args }.data(),
            },
        ))
    }

    pub fn add_queue_authority_ix(
        payer: Pubkey,
        task_queue_key: Pubkey,
        queue_authority: Pubkey,
        update_authority: Pubkey,
    ) -> Result<Instruction, Error> {
        Ok(Instruction {
            program_id: tuktuk::ID,
            accounts: tuktuk::client::accounts::AddQueueAuthorityV0 {
                task_queue: task_queue_key,
                queue_authority,
                payer,
                update_authority,
                task_queue_authority: task_queue_authority_key(&task_queue_key, &queue_authority),
                system_program: solana_sdk::system_program::ID,
            }
            .to_account_metas(None),
            data: tuktuk::client::args::AddQueueAuthorityV0 {}.data(),
        })
    }

    pub async fn add_queue_authority<C: GetAnchorAccount>(
        client: &C,
        payer: Pubkey,
        task_queue_key: Pubkey,
        queue_authority: Pubkey,
    ) -> Result<Instruction, Error> {
        let task_queue: TaskQueueV0 = client
            .anchor_account(&task_queue_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        add_queue_authority_ix(
            payer,
            task_queue_key,
            queue_authority,
            task_queue.update_authority,
        )
    }

    pub fn remove_queue_authority_ix(
        payer: Pubkey,
        rent_refund: Pubkey,
        task_queue_key: Pubkey,
        queue_authority: Pubkey,
        update_authority: Pubkey,
    ) -> Result<Instruction, Error> {
        Ok(Instruction {
            program_id: tuktuk::ID,
            accounts: tuktuk::client::accounts::RemoveQueueAuthorityV0 {
                task_queue: task_queue_key,
                queue_authority,
                payer,
                update_authority,
                task_queue_authority: task_queue_authority_key(&task_queue_key, &queue_authority),
                rent_refund,
            }
            .to_account_metas(None),
            data: tuktuk::client::args::RemoveQueueAuthorityV0 {}.data(),
        })
    }

    pub async fn remove_queue_authority<C: GetAnchorAccount>(
        client: &C,
        payer: Pubkey,
        task_queue_key: Pubkey,
        queue_authority: Pubkey,
    ) -> Result<Instruction, Error> {
        let task_queue: TaskQueueV0 = client
            .anchor_account(&task_queue_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        remove_queue_authority_ix(
            payer,
            payer,
            task_queue_key,
            queue_authority,
            task_queue.update_authority,
        )
    }

    pub async fn close<C: GetAnchorAccount>(
        client: &C,
        task_queue_key: Pubkey,
        payer: Pubkey,
        rent_refund: Pubkey,
    ) -> Result<Instruction, Error> {
        let config_key = config_key();
        let queue: TaskQueueV0 = client
            .anchor_account(&task_queue_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        Ok(Instruction {
            program_id: tuktuk::ID,
            accounts: tuktuk::client::accounts::CloseTaskQueueV0 {
                task_queue: task_queue_key,
                rent_refund,
                task_queue_name_mapping: task_queue_name_mapping_key(&config_key, &queue.name),
                payer,
                system_program: solana_sdk::system_program::ID,
                tuktuk_config: config_key,
                update_authority: queue.update_authority,
            }
            .to_account_metas(None),
            data: tuktuk::client::args::CloseTaskQueueV0 {}.data(),
        })
    }

    pub async fn update<C: GetAnchorAccount>(
        client: &C,
        payer: Pubkey,
        task_queue_key: Pubkey,
        args: UpdateTaskQueueArgsV0,
    ) -> Result<Instruction, Error> {
        let task_queue: TaskQueueV0 = client
            .anchor_account(&task_queue_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        update_ix(
            payer,
            task_queue_key,
            Some(task_queue.update_authority),
            args,
        )
    }

    pub fn update_ix(
        payer: Pubkey,
        task_queue_key: Pubkey,
        update_authority: Option<Pubkey>,
        args: UpdateTaskQueueArgsV0,
    ) -> Result<Instruction, Error> {
        Ok(Instruction {
            program_id: tuktuk::ID,
            accounts: tuktuk::client::accounts::UpdateTaskQueueV0 {
                task_queue: task_queue_key,
                payer,
                system_program: solana_sdk::system_program::ID,
                update_authority: update_authority.unwrap_or(payer),
            }
            .to_account_metas(None),
            data: tuktuk::client::args::UpdateTaskQueueV0 { args }.data(),
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
    pub task_queue: TaskQueueV0,
    pub removed: Vec<Pubkey>,
}

pub mod task {
    use std::sync::Arc;

    use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
    use futures::{future::BoxFuture, Stream, StreamExt};
    use itertools::Itertools;
    use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
    use tokio::sync::Mutex;
    use tuktuk_program::TaskV0;

    use super::{
        task_queue::task_queue_authority_key,
        tuktuk::{self, accounts::TaskQueueV0, ID},
        types::QueueTaskArgsV0,
        TaskUpdate,
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
            .filter(|k| task_queue.task_exists(*k))
            .map(|id| self::key(queue_key, id))
            .collect_vec();
        Ok(task_keys)
    }

    pub fn queue_ix(
        task_queue_key: Pubkey,
        task_queue: &TaskQueueV0,
        payer: Pubkey,
        queue_authority: Pubkey,
        args: QueueTaskArgsV0,
    ) -> Result<(Pubkey, Instruction), Error> {
        let task_key = self::key(
            &task_queue_key,
            task_queue
                .next_available_task_id()
                .ok_or_else(|| Error::TooManyTasks)?,
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
                    task_queue_authority: task_queue_authority_key(
                        &task_queue_key,
                        &queue_authority,
                    ),
                    queue_authority,
                }
                .to_account_metas(None),
                data: tuktuk::client::args::QueueTaskV0 { args }.data(),
            },
        ))
    }

    pub async fn queue<C: GetAnchorAccount>(
        client: &C,
        payer: Pubkey,
        queue_authority: Pubkey,
        task_queue_key: Pubkey,
        args: QueueTaskArgsV0,
    ) -> Result<(Pubkey, Instruction), Error> {
        let task_queue: TaskQueueV0 = client
            .anchor_account(&task_queue_key)
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        self::queue_ix(task_queue_key, &task_queue, payer, queue_authority, args)
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
                    .filter(|id| new_task_queue.task_exists(*id) && !last_tq_clone.task_exists(*id))
                    .map(|id| self::key(task_queue_key, id))
                    .collect_vec();

                let removed_task_keys = task_ids
                    .clone()
                    .filter(|id| !new_task_queue.task_exists(*id) && last_tq_clone.task_exists(*id))
                    .map(|id| self::key(task_queue_key, id))
                    .collect_vec();

                let tasks = client.anchor_accounts(&new_task_keys).await?;
                Ok(TaskUpdate {
                    tasks,
                    task_queue: new_task_queue,
                    removed: removed_task_keys,
                })
            }
        });

        Ok((result, unsubscribe))
    }

    pub fn dequeue_ix(
        task_queue_key: Pubkey,
        queue_authority: Pubkey,
        rent_refund: Pubkey,
        index: u16,
    ) -> Result<Instruction, Error> {
        Ok(Instruction {
            program_id: ID,
            accounts: tuktuk::client::accounts::DequeueTaskV0 {
                task_queue: task_queue_key,
                rent_refund,
                task: self::key(&task_queue_key, index),
                task_queue_authority: task_queue_authority_key(&task_queue_key, &queue_authority),
                queue_authority,
            }
            .to_account_metas(None),
            data: tuktuk::client::args::DequeueTaskV0 {}.data(),
        })
    }

    pub async fn dequeue<C: GetAnchorAccount>(
        client: &C,
        task_queue_key: Pubkey,
        queue_authority: Pubkey,
        index: u16,
    ) -> Result<Instruction, Error> {
        let task: TaskV0 = client
            .anchor_account(&self::key(&task_queue_key, index))
            .await?
            .ok_or_else(|| Error::AccountNotFound)?;

        self::dequeue_ix(task_queue_key, queue_authority, task.rent_refund, index)
    }
}
