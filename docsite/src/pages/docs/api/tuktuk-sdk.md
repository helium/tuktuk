# Tuktuk SDK

{% callout title="Quick tip" %}
If you are looking for a quick start guide, check out the [Quickstart](/docs/learn/quickstart) guide.
{% /callout %}

## Instructions

### add_queue_authority_v0

#### Accounts

| Name                 | Mutability | Signer | Docs |
| -------------------- | ---------- | ------ | ---- |
| payer                | mut        | yes    |      |
| update_authority     | immut      | yes    |      |
| queue_authority      | immut      | no     |      |
| task_queue_authority | mut        | no     |      |
| task_queue           | mut        | no     |      |
| system_program       | immut      | no     |      |

### close_task_queue_v0

#### Accounts

| Name                    | Mutability | Signer | Docs |
| ----------------------- | ---------- | ------ | ---- |
| rent_refund             | mut        | no     |      |
| payer                   | mut        | yes    |      |
| update_authority        | immut      | yes    |      |
| tuktuk_config           | mut        | no     |      |
| task_queue              | mut        | no     |      |
| task_queue_name_mapping | mut        | no     |      |
| system_program          | immut      | no     |      |

### dequeue_task_v0

#### Accounts

| Name                 | Mutability | Signer | Docs |
| -------------------- | ---------- | ------ | ---- |
| queue_authority      | immut      | yes    |      |
| rent_refund          | mut        | no     |      |
| task_queue_authority | immut      | no     |      |
| task_queue           | mut        | no     |      |
| task                 | mut        | no     |      |

### dummy_ix

#### Accounts

| Name  | Mutability | Signer | Docs |
| ----- | ---------- | ------ | ---- |
| dummy | mut        | no     |      |

### initialize_task_queue_v0

#### Accounts

| Name                    | Mutability | Signer | Docs |
| ----------------------- | ---------- | ------ | ---- |
| payer                   | mut        | yes    |      |
| tuktuk_config           | mut        | no     |      |
| update_authority        | immut      | no     |      |
| task_queue              | mut        | no     |      |
| task_queue_name_mapping | mut        | no     |      |
| system_program          | immut      | no     |      |

#### Args

| Name | Type                      | Docs |
| ---- | ------------------------- | ---- |
| args | InitializeTaskQueueArgsV0 |      |

**InitializeTaskQueueArgsV0 Fields:**

| Field            | Type        | Description |
| ---------------- | ----------- | ----------- |
| min_crank_reward | u64         |             |
| name             | string      |             |
| capacity         | u16         |             |
| lookup_tables    | Vec<pubkey> |             |
| stale_task_age   | u32         |             |

### initialize_tuktuk_config_v0

#### Accounts

| Name           | Mutability | Signer | Docs |
| -------------- | ---------- | ------ | ---- |
| payer          | mut        | yes    |      |
| approver       | immut      | yes    |      |
| authority      | immut      | no     |      |
| tuktuk_config  | mut        | no     |      |
| system_program | immut      | no     |      |

#### Args

| Name | Type                         | Docs |
| ---- | ---------------------------- | ---- |
| args | InitializeTuktukConfigArgsV0 |      |

**InitializeTuktukConfigArgsV0 Fields:**

| Field       | Type | Description |
| ----------- | ---- | ----------- |
| min_deposit | u64  |             |

### queue_task_v0

#### Accounts

| Name                 | Mutability | Signer | Docs |
| -------------------- | ---------- | ------ | ---- |
| payer                | mut        | yes    |      |
| queue_authority      | immut      | yes    |      |
| task_queue_authority | immut      | no     |      |
| task_queue           | mut        | no     |      |
| task                 | mut        | no     |      |
| system_program       | immut      | no     |      |

#### Args

| Name | Type            | Docs |
| ---- | --------------- | ---- |
| args | QueueTaskArgsV0 |      |

**QueueTaskArgsV0 Fields:**

| Field        | Type                | Description |
| ------------ | ------------------- | ----------- |
| id           | u16                 |             |
| trigger      | TriggerV0           |             |
| transaction  | TransactionSourceV0 |             |
| crank_reward | Option<u64>         |             |
| free_tasks   | u8                  |             |
| description  | string              |             |

### remove_queue_authority_v0

#### Accounts

| Name                 | Mutability | Signer | Docs |
| -------------------- | ---------- | ------ | ---- |
| payer                | mut        | yes    |      |
| rent_refund          | mut        | no     |      |
| update_authority     | immut      | yes    |      |
| queue_authority      | immut      | no     |      |
| task_queue_authority | mut        | no     |      |
| task_queue           | mut        | no     |      |

### return_tasks_v0

#### Accounts

| Name           | Mutability | Signer | Docs |
| -------------- | ---------- | ------ | ---- |
| system_program | immut      | no     |      |

#### Args

| Name | Type              | Docs |
| ---- | ----------------- | ---- |
| args | ReturnTasksArgsV0 |      |

**ReturnTasksArgsV0 Fields:**

| Field | Type              | Description |
| ----- | ----------------- | ----------- |
| tasks | Vec<TaskReturnV0> |             |

### run_task_v0

#### Accounts

| Name                | Mutability | Signer | Docs                                                                                                                                                   |
| ------------------- | ---------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| crank_turner        | mut        | yes    |                                                                                                                                                        |
| rent_refund         | mut        | no     |                                                                                                                                                        |
| task_queue          | mut        | no     |                                                                                                                                                        |
| task                | mut        | no     |                                                                                                                                                        |
| system_program      | immut      | no     |                                                                                                                                                        |
| sysvar_instructions | immut      | no     | the supplied Sysvar could be anything else. The Instruction Sysvar has not been implemented in the Anchor framework yet, so this is the safe approach. |

#### Args

| Name | Type          | Docs |
| ---- | ------------- | ---- |
| args | RunTaskArgsV0 |      |

**RunTaskArgsV0 Fields:**

| Field         | Type     | Description |
| ------------- | -------- | ----------- |
| free_task_ids | Vec<u16> |             |

### update_task_queue_v0

#### Accounts

| Name             | Mutability | Signer | Docs |
| ---------------- | ---------- | ------ | ---- |
| payer            | mut        | yes    |      |
| update_authority | immut      | yes    |      |
| task_queue       | mut        | no     |      |
| system_program   | immut      | no     |      |

#### Args

| Name | Type                  | Docs |
| ---- | --------------------- | ---- |
| args | UpdateTaskQueueArgsV0 |      |

**UpdateTaskQueueArgsV0 Fields:**

| Field            | Type            | Description |
| ---------------- | --------------- | ----------- |
| min_crank_reward | Option<u64>     |             |
| capacity         | Option<u16>     |             |
| lookup_tables    | Option<unknown> |             |
| update_authority | Option<pubkey>  |             |
| stale_task_age   | Option<u32>     |             |

## Accounts

### RemoteTaskTransactionV0

| Field             | Type                  | Description |
| ----------------- | --------------------- | ----------- |
| verification_hash | [u8; 32]              |             |
| transaction       | CompiledTransactionV0 |             |

### TaskQueueAuthorityV0

| Field           | Type   | Description |
| --------------- | ------ | ----------- |
| task_queue      | pubkey |             |
| queue_authority | pubkey |             |
| bump_seed       | u8     |             |

### TaskQueueNameMappingV0

| Field      | Type   | Description |
| ---------- | ------ | ----------- |
| task_queue | pubkey |             |
| name       | string |             |
| bump_seed  | u8     |             |

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

### TaskV0

| Field        | Type                | Description |
| ------------ | ------------------- | ----------- |
| task_queue   | pubkey              |             |
| rent_amount  | u64                 |             |
| crank_reward | u64                 |             |
| id           | u16                 |             |
| trigger      | TriggerV0           |             |
| rent_refund  | pubkey              |             |
| transaction  | TransactionSourceV0 |             |
| queued_at    | i64                 |             |
| bump_seed    | u8                  |             |
| free_tasks   | u8                  |             |
| description  | string              |             |

### TuktukConfigV0

| Field              | Type   | Description |
| ------------------ | ------ | ----------- |
| min_task_queue_id  | u32    |             |
| next_task_queue_id | u32    |             |
| authority          | pubkey |             |
| min_deposit        | u64    |             |
| bump_seed          | u8     |             |

## Types

### CompiledInstructionV0

| Field            | Type  | Description                                                                                          |
| ---------------- | ----- | ---------------------------------------------------------------------------------------------------- |
| program_id_index | u8    | Index into the transaction keys array indicating the program account that executes this instruction. |
| accounts         | bytes | Ordered indices into the transaction keys array indicating which accounts to pass to the program.    |
| data             | bytes | The program input data.                                                                              |

### CompiledTransactionV0

| Field          | Type                       | Description                                                                                                                                                                                                                                                                                                                                                 |
| -------------- | -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| num_rw_signers | u8                         |                                                                                                                                                                                                                                                                                                                                                             |
| num_ro_signers | u8                         |                                                                                                                                                                                                                                                                                                                                                             |
| num_rw         | u8                         |                                                                                                                                                                                                                                                                                                                                                             |
| accounts       | Vec<pubkey>                |                                                                                                                                                                                                                                                                                                                                                             |
| instructions   | Vec<CompiledInstructionV0> |                                                                                                                                                                                                                                                                                                                                                             |
| signer_seeds   | Vec<unknown>               | Additional signer seeds. Should include bump. Useful for things like initializing a mint where you cannot pass a keypair. Note that these seeds will be prefixed with "custom", task_queue.key and the bump you pass and account should be consistent with this. But to save space in the instruction, they should be ommitted here. See tests for examples |

### InitializeTaskQueueArgsV0

| Field            | Type        | Description |
| ---------------- | ----------- | ----------- |
| min_crank_reward | u64         |             |
| name             | string      |             |
| capacity         | u16         |             |
| lookup_tables    | Vec<pubkey> |             |
| stale_task_age   | u32         |             |

### InitializeTuktukConfigArgsV0

| Field       | Type | Description |
| ----------- | ---- | ----------- |
| min_deposit | u64  |             |

### QueueTaskArgsV0

| Field        | Type                | Description |
| ------------ | ------------------- | ----------- |
| id           | u16                 |             |
| trigger      | TriggerV0           |             |
| transaction  | TransactionSourceV0 |             |
| crank_reward | Option<u64>         |             |
| free_tasks   | u8                  |             |
| description  | string              |             |

### RemoteTaskTransactionV0

| Field             | Type                  | Description |
| ----------------- | --------------------- | ----------- |
| verification_hash | [u8; 32]              |             |
| transaction       | CompiledTransactionV0 |             |

### ReturnTasksArgsV0

| Field | Type              | Description |
| ----- | ----------------- | ----------- |
| tasks | Vec<TaskReturnV0> |             |

### RunTaskArgsV0

| Field         | Type     | Description |
| ------------- | -------- | ----------- |
| free_task_ids | Vec<u16> |             |

### RunTaskReturnV0

| Field          | Type              | Description |
| -------------- | ----------------- | ----------- |
| tasks          | Vec<TaskReturnV0> |             |
| tasks_accounts | Vec<pubkey>       |             |

### TaskQueueAuthorityV0

| Field           | Type   | Description |
| --------------- | ------ | ----------- |
| task_queue      | pubkey |             |
| queue_authority | pubkey |             |
| bump_seed       | u8     |             |

### TaskQueueNameMappingV0

| Field      | Type   | Description |
| ---------- | ------ | ----------- |
| task_queue | pubkey |             |
| name       | string |             |
| bump_seed  | u8     |             |

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

### TaskV0

| Field        | Type                | Description |
| ------------ | ------------------- | ----------- |
| task_queue   | pubkey              |             |
| rent_amount  | u64                 |             |
| crank_reward | u64                 |             |
| id           | u16                 |             |
| trigger      | TriggerV0           |             |
| rent_refund  | pubkey              |             |
| transaction  | TransactionSourceV0 |             |
| queued_at    | i64                 |             |
| bump_seed    | u8                  |             |
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

### TuktukConfigV0

| Field              | Type   | Description |
| ------------------ | ------ | ----------- |
| min_task_queue_id  | u32    |             |
| next_task_queue_id | u32    |             |
| authority          | pubkey |             |
| min_deposit        | u64    |             |
| bump_seed          | u8     |             |

### UpdateTaskQueueArgsV0

| Field            | Type            | Description |
| ---------------- | --------------- | ----------- |
| min_crank_reward | Option<u64>     |             |
| capacity         | Option<u16>     |             |
| lookup_tables    | Option<unknown> |             |
| update_authority | Option<pubkey>  |             |
| stale_task_age   | Option<u32>     |             |

## Errors

| Code | Name                            | Message                                           |
| ---- | ------------------------------- | ------------------------------------------------- |
| 6000 | TaskAlreadyExists               | Task already exists                               |
| 6001 | InvalidSigner                   | Signer account mismatched account in definition   |
| 6002 | InvalidWritable                 | Writable account mismatched account in definition |
| 6003 | InvalidAccount                  | Account mismatched account in definition          |
| 6004 | InvalidDataIncrease             | Invalid data increase                             |
| 6005 | TaskNotReady                    | Task not ready                                    |
| 6006 | TaskQueueNotEmpty               | Task queue not empty                              |
| 6007 | FreeTaskAccountNotEmpty         | Free task account not empty                       |
| 6008 | InvalidTaskPDA                  | Invalid task PDA                                  |
| 6009 | TaskQueueInsufficientFunds      | Task queue insufficient funds                     |
| 6010 | SigVerificationFailed           | Sig verification failed                           |
| 6011 | InvalidTransactionSource        | Invalid transaction source                        |
| 6012 | InvalidVerificationAccountsHash | Invalid task verification hash                    |
| 6013 | InvalidRentRefund               | Invalid rent refund                               |
| 6014 | InvalidTaskId                   | Invalid task id                                   |
| 6015 | DummyInstruction                | Don't use the dummy instruction                   |
| 6016 | InvalidDescriptionLength        | Invalid description length                        |
| 6017 | TaskQueueHasQueueAuthorities    | Task queue has queue authorities                  |
