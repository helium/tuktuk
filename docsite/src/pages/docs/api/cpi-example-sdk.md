# Cpi Example SDK

{% callout title="Quick tip" %}
If you are looking for a quick start guide, check out the [Quickstart](/docs/learn/quickstart) guide.
{% /callout %}

## Instructions

### recurring_task

#### Accounts

| Name           | Mutability | Signer | Docs |
| -------------- | ---------- | ------ | ---- |
| system_program | immut      | no     |      |

### recurring_task_with_account_return

#### Accounts

| Name                | Mutability | Signer | Docs |
| ------------------- | ---------- | ------ | ---- |
| queue_authority     | mut        | no     |      |
| system_program      | immut      | no     |      |
| task_return_account | mut        | no     |      |

### schedule

#### Accounts

| Name                 | Mutability | Signer | Docs |
| -------------------- | ---------- | ------ | ---- |
| task_queue           | mut        | no     |      |
| task_queue_authority | immut      | no     |      |
| task                 | mut        | no     |      |
| queue_authority      | mut        | no     |      |
| system_program       | immut      | no     |      |
| tuktuk_program       | immut      | no     |      |

#### Args

| Name    | Type    | Docs |
| ------- | ------- | ---- |
| task_id | Unknown |      |

### schedule_with_account_return

#### Accounts

| Name                 | Mutability | Signer | Docs |
| -------------------- | ---------- | ------ | ---- |
| task_queue           | mut        | no     |      |
| task_queue_authority | immut      | no     |      |
| task                 | mut        | no     |      |
| queue_authority      | mut        | no     |      |
| task_return_account  | immut      | no     |      |
| system_program       | immut      | no     |      |
| tuktuk_program       | immut      | no     |      |

#### Args

| Name    | Type    | Docs |
| ------- | ------- | ---- |
| task_id | Unknown |      |

## Types

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

### RunTaskReturnV0

| Field    | Type              | Description |
| -------- | ----------------- | ----------- |
| tasks    | Vec<TaskReturnV0> |             |
| accounts | Vec<pubkey>       |             |

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
