# Cron SDK

{% callout title="Quick tip" %}
If you are looking for a quick start guide, check out the [Quickstart](/docs/learn/quickstart) guide.
{% /callout %}

## Instructions

### add_cron_transaction_v0

#### Accounts

| Name                 | Mutability | Signer | Docs |
| -------------------- | ---------- | ------ | ---- |
| payer                | mut        | yes    |      |
| authority            | immut      | yes    |      |
| cron_job             | mut        | no     |      |
| cron_job_transaction | mut        | no     |      |
| system_program       | immut      | no     |      |

#### Args

| Name | Type                     | Docs |
| ---- | ------------------------ | ---- |
| args | AddCronTransactionArgsV0 |      |

**AddCronTransactionArgsV0 Fields:**

| Field              | Type                | Description |
| ------------------ | ------------------- | ----------- |
| index              | u32                 |             |
| transaction_source | TransactionSourceV0 |             |

### close_cron_job_v0

#### Accounts

| Name                  | Mutability | Signer | Docs |
| --------------------- | ---------- | ------ | ---- |
| rent_refund           | mut        | no     |      |
| authority             | immut      | yes    |      |
| user_cron_jobs        | mut        | no     |      |
| cron_job              | mut        | no     |      |
| cron_job_name_mapping | mut        | no     |      |
| system_program        | immut      | no     |      |
| task_return_account_1 | mut        | no     |      |
| task_return_account_2 | mut        | no     |      |

### initialize_cron_job_v0

#### Accounts

| Name                  | Mutability | Signer | Docs |
| --------------------- | ---------- | ------ | ---- |
| payer                 | mut        | yes    |      |
| queue_authority       | immut      | yes    |      |
| task_queue_authority  | immut      | no     |      |
| authority             | immut      | yes    |      |
| user_cron_jobs        | mut        | no     |      |
| cron_job              | mut        | no     |      |
| cron_job_name_mapping | mut        | no     |      |
| task_queue            | mut        | no     |      |
| task                  | mut        | no     |      |
| task_return_account_1 | mut        | no     |      |
| task_return_account_2 | mut        | no     |      |
| system_program        | immut      | no     |      |
| tuktuk_program        | immut      | no     |      |

#### Args

| Name | Type                    | Docs |
| ---- | ----------------------- | ---- |
| args | InitializeCronJobArgsV0 |      |

**InitializeCronJobArgsV0 Fields:**

| Field                      | Type   | Description                                                                                                                                                                                                                                                                                                                                                                                                                      |
| -------------------------- | ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| schedule                   | string |                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| name                       | string |                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| free_tasks_per_transaction | u8     | The number of free tasks each transaction will be executed with. This allows transactions scheduled via cron to themselves schedule more transactions. If none of your transactions need to schedule more transactions, set this to 0.                                                                                                                                                                                           |
| num_tasks_per_queue_call   | u8     | The number of tasks to queue per queue call. Cron job works by queueing a single task that runs at the appropriate time. This tasks job is to recursively queue all transactions in this cron. The higher you set this number, the more tasks will be queued per queue call, making the tasks execute faster/more parallelized. Setting this too high without proper lookup tables will result in the queue call being too large |

### queue_cron_tasks_v0

#### Accounts

| Name                  | Mutability | Signer | Docs |
| --------------------- | ---------- | ------ | ---- |
| cron_job              | mut        | no     |      |
| task_queue            | immut      | no     |      |
| task_return_account_1 | mut        | no     |      |
| task_return_account_2 | mut        | no     |      |
| system_program        | immut      | no     |      |

### remove_cron_transaction_v0

#### Accounts

| Name                 | Mutability | Signer | Docs |
| -------------------- | ---------- | ------ | ---- |
| rent_refund          | mut        | yes    |      |
| authority            | immut      | yes    |      |
| cron_job             | mut        | no     |      |
| cron_job_transaction | mut        | no     |      |
| system_program       | immut      | no     |      |

#### Args

| Name | Type                        | Docs |
| ---- | --------------------------- | ---- |
| args | RemoveCronTransactionArgsV0 |      |

**RemoveCronTransactionArgsV0 Fields:**

| Field | Type | Description |
| ----- | ---- | ----------- |
| index | u32  |             |

## Accounts

### CronJobNameMappingV0

| Field     | Type   | Description |
| --------- | ------ | ----------- |
| cron_job  | pubkey |             |
| name      | string |             |
| bump_seed | u8     |             |

### CronJobTransactionV0

| Field       | Type                | Description |
| ----------- | ------------------- | ----------- |
| id          | u32                 |             |
| cron_job    | pubkey              |             |
| transaction | TransactionSourceV0 |             |
| bump_seed   | u8                  |             |

### CronJobV0

| Field                      | Type   | Description |
| -------------------------- | ------ | ----------- |
| id                         | u32    |             |
| user_cron_jobs             | pubkey |             |
| task_queue                 | pubkey |             |
| authority                  | pubkey |             |
| free_tasks_per_transaction | u8     |             |
| num_tasks_per_queue_call   | u8     |             |
| schedule                   | string |             |
| name                       | string |             |
| current_exec_ts            | i64    |             |
| current_transaction_id     | u32    |             |
| num_transactions           | u32    |             |
| next_transaction_id        | u32    |             |
| removed_from_queue         | bool   |             |
| bump_seed                  | u8     |             |

### TaskQueueAuthorityV0

| Field           | Type   | Description |
| --------------- | ------ | ----------- |
| task_queue      | pubkey |             |
| queue_authority | pubkey |             |
| bump_seed       | u8     |             |

### TaskQueueV0

| Field                     | Type        | Description |
| ------------------------- | ----------- | ----------- |
| tuktuk_config             | pubkey      |             |
| id                        | u32         |             |
| update_authority          | pubkey      |             |
| reserved                  | pubkey      |             |
| min_crank_reward          | u64         |             |
| uncollected_protocol_fees | u64         |             |
| capacity                  | u16         |             |
| created_at                | i64         |             |
| updated_at                | i64         |             |
| bump_seed                 | u8          |             |
| task_bitmap               | bytes       |             |
| name                      | string      |             |
| lookup_tables             | Vec<pubkey> |             |
| num_queue_authorities     | u16         |             |
| stale_task_age            | u32         |             |

### UserCronJobsV0

| Field            | Type   | Description |
| ---------------- | ------ | ----------- |
| authority        | pubkey |             |
| min_cron_job_id  | u32    |             |
| next_cron_job_id | u32    |             |
| bump_seed        | u8     |             |

## Types

### AddCronTransactionArgsV0

| Field              | Type                | Description |
| ------------------ | ------------------- | ----------- |
| index              | u32                 |             |
| transaction_source | TransactionSourceV0 |             |

### CompiledInstructionV0

| Field            | Type  | Description |
| ---------------- | ----- | ----------- |
| program_id_index | u8    |             |
| accounts         | bytes |             |
| data             | bytes |             |

### CompiledTransactionV0

| Field          | Type                       | Description |
| -------------- | -------------------------- | ----------- |
| num_rw_signers | u8                         |             |
| num_ro_signers | u8                         |             |
| num_rw         | u8                         |             |
| accounts       | Vec<pubkey>                |             |
| instructions   | Vec<CompiledInstructionV0> |             |
| signer_seeds   | Vec<unknown>               |             |

### CronJobNameMappingV0

| Field     | Type   | Description |
| --------- | ------ | ----------- |
| cron_job  | pubkey |             |
| name      | string |             |
| bump_seed | u8     |             |

### CronJobTransactionV0

| Field       | Type                | Description |
| ----------- | ------------------- | ----------- |
| id          | u32                 |             |
| cron_job    | pubkey              |             |
| transaction | TransactionSourceV0 |             |
| bump_seed   | u8                  |             |

### CronJobV0

| Field                      | Type   | Description |
| -------------------------- | ------ | ----------- |
| id                         | u32    |             |
| user_cron_jobs             | pubkey |             |
| task_queue                 | pubkey |             |
| authority                  | pubkey |             |
| free_tasks_per_transaction | u8     |             |
| num_tasks_per_queue_call   | u8     |             |
| schedule                   | string |             |
| name                       | string |             |
| current_exec_ts            | i64    |             |
| current_transaction_id     | u32    |             |
| num_transactions           | u32    |             |
| next_transaction_id        | u32    |             |
| removed_from_queue         | bool   |             |
| bump_seed                  | u8     |             |

### InitializeCronJobArgsV0

| Field                      | Type   | Description                                                                                                                                                                                                                                                                                                                                                                                                                      |
| -------------------------- | ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| schedule                   | string |                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| name                       | string |                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| free_tasks_per_transaction | u8     | The number of free tasks each transaction will be executed with. This allows transactions scheduled via cron to themselves schedule more transactions. If none of your transactions need to schedule more transactions, set this to 0.                                                                                                                                                                                           |
| num_tasks_per_queue_call   | u8     | The number of tasks to queue per queue call. Cron job works by queueing a single task that runs at the appropriate time. This tasks job is to recursively queue all transactions in this cron. The higher you set this number, the more tasks will be queued per queue call, making the tasks execute faster/more parallelized. Setting this too high without proper lookup tables will result in the queue call being too large |

### RemoveCronTransactionArgsV0

| Field | Type | Description |
| ----- | ---- | ----------- |
| index | u32  |             |

### RunTaskReturnV0

| Field    | Type              | Description |
| -------- | ----------------- | ----------- |
| tasks    | Vec<TaskReturnV0> |             |
| accounts | Vec<pubkey>       |             |

### TaskQueueAuthorityV0

| Field           | Type   | Description |
| --------------- | ------ | ----------- |
| task_queue      | pubkey |             |
| queue_authority | pubkey |             |
| bump_seed       | u8     |             |

### TaskQueueV0

| Field                     | Type        | Description |
| ------------------------- | ----------- | ----------- |
| tuktuk_config             | pubkey      |             |
| id                        | u32         |             |
| update_authority          | pubkey      |             |
| reserved                  | pubkey      |             |
| min_crank_reward          | u64         |             |
| uncollected_protocol_fees | u64         |             |
| capacity                  | u16         |             |
| created_at                | i64         |             |
| updated_at                | i64         |             |
| bump_seed                 | u8          |             |
| task_bitmap               | bytes       |             |
| name                      | string      |             |
| lookup_tables             | Vec<pubkey> |             |
| num_queue_authorities     | u16         |             |
| stale_task_age            | u32         |             |

### TaskReturnV0

| Field        | Type                | Description |
| ------------ | ------------------- | ----------- |
| trigger      | TriggerV0           |             |
| transaction  | TransactionSourceV0 |             |
| crank_reward | Option<u64>         |             |
| free_tasks   | u8                  |             |
| description  | string              |             |

### TransactionSourceV0

| Variant    | Fields                      | Description |
| ---------- | --------------------------- | ----------- |
| CompiledV0 | unknown                     |             |
| RemoteV0   | url: string, signer: pubkey |             |

### TriggerV0

| Variant   | Fields | Description |
| --------- | ------ | ----------- |
| Now       |        |             |
| Timestamp | i64    |             |

### UserCronJobsV0

| Field            | Type   | Description |
| ---------------- | ------ | ----------- |
| authority        | pubkey |             |
| min_cron_job_id  | u32    |             |
| next_cron_job_id | u32    |             |
| bump_seed        | u8     |             |

## Errors

| Code | Name                        | Message                                |
| ---- | --------------------------- | -------------------------------------- |
| 6000 | InvalidSchedule             | Invalid schedule                       |
| 6001 | TransactionAlreadyExists    | Transaction already exists             |
| 6002 | InsufficientFunds           | Insufficient funds                     |
| 6003 | Overflow                    | Overflow                               |
| 6004 | InvalidDataIncrease         | Invalid data increase                  |
| 6005 | CronJobHasTransactions      | Cron job has transactions              |
| 6006 | InvalidNumTasksPerQueueCall | Invalid number of tasks per queue call |
| 6007 | TooEarly                    | Too early to queue tasks               |
