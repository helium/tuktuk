# TukTuk

Run your permissionless cranks on Solana

![TukTuk](./tuktuk.jpg)

## Table of Contents
- [Introduction](#introduction)
- [Running a Crank Turner](#running-a-crank-turner)
  - [Requirements](#requirements)
- [Usage](#usage)
  - [Prerequisites](#prerequisites)
  - [Installation](#installation)
  - [Quickstart](#quickstart)
  - [Create a Task Queue](#create-a-task-queue)
  - [Funding a Task Queue](#funding-a-task-queue)
  - [Adding Queue Authorities](#adding-queue-authorities)
  - [Queue a Task](#queue-a-task)
  - [Remote Transactions](#remote-transactions)
  - [Monitoring the Task Queue](#monitoring-the-task-queue)
  - [Cron Tasks](#cron-tasks)
  - [Monitoring the Cron Job](#monitoring-the-cron-job)
  - [Running a Task](#running-a-task)
  - [Closing Tasks](#closing-tasks)
- [Development Setup](#development-setup)
- [Troubleshooting](#troubleshooting)
  - [Common Issues and Solutions](#common-issues-and-solutions)
    - [Account Not Initialized Error](#account-not-initialized-error)
    - [Task Not Running](#task-not-running)
    - [Cron Job Stopped Running](#cron-job-stopped-running)
    - [Task Fixed But Not Running](#task-fixed-but-not-running)

## Introduction

Tuktuk is a permissionless crank service. If you have a Solana smart contract endpoint that needs to be run on a trigger or specific time, you can use tuktuk to run it. Endponts need to be more or less permissionless, though you can have tuktuk provide PDA signatures.

Tuktuk's architecture allows for crankers to run a simple rust util that requires only a working solana RPC url and very minimal dependencies. There is no dependency on geyser, yellowstone, or any other indexing service.

Creators of Task Queues set their payment per-crank turn in SOL. Crankers that run the tasks are paid out in SOL for each crank they complete. There is a minimum deposit of 1 SOL to create a task queue to discourage spam. This deposit is refunded when the task queue is closed. The intent is to minimize the number of task queues that crank turners need to watch. You should try to reuse task queues as much as possible. It is an antipattern to create a new task queue for each user, for example.

## Running a Crank Turner

Install the crank turner:

```bash
cargo install tuktuk-crank-turner
```

If you want to run a crank turner, create a config.toml file with the following:

```toml
rpc_url = "https://api.mainnet-beta.solana.com"
key_path = "/path/to/your/keypair.json"
min_crank_fee = 10000
```

Then run the crank turner:

```bash
tuktuk-crank-turner -c config.toml
```

You can also provider configuration via environment variables

```bash
export TUKTUK__RPC_URL="https://api.mainnet-beta.solana.com"
export TUKTUK__KEY_PATH="/path/to/your/keypair.json"
export TUKTUK__MIN_CRANK_FEE=10000
tuktuk-crank-turner
```

### Requirements

You will need a good Solana RPC that doesn't have heavy rate limits (for when there are a lot of tasks queued). You should also handle restarting the process if it crashes, as this can happen if your RPC disconnects the websocket without a proper handshake.

## Usage

First, you'll want to install the tuktuk-cli. The cli is great for debugging and managing your task queue.

### Prerequisites

Make sure you are on rustc 1.85:

```bash
rustup install 1.85
rustup default 1.85
```

Make sure you also have openssl installed:

```bash
brew install openssl
```

### Installation 

Install the tuktuk cli by running:

```bash
cargo install tuktuk-cli
```

### Quickstart

For quick examples of how to queue tasks and crons using tuktuk in typescript, see the [typescript-examples](./typescript-examples) folder.

### Create a task queue

First, you'll need to get some SOL to fund the task queue. You can get SOL from [Jupiter Aggregator](https://www.jup.ag/swap/USDC-hntyVP6YFm1Hg25TN9WGLqM12b8TQmcknKrdu1oxWux).

Next, create a task queue. A task queue has a default crank reward that will be used for all tasks in the queue, but each task can override this reward. Since crankers pay sol (and possibly priority fees) for each crank, the crank reward should be higher than the cost of a crank or crankers will not be incentivized to run your task.

Note that the `funding-amount` you specify is not inclusive of the 1 SOL minimum deposit. The funding amount will be used to pay the fees for tasks queued recursively (ie, by other tasks). 

```bash
tuktuk -u <your-solana-url> task-queue create --name <your-queue-name> --capacity 10 --funding-amount 100000000 --queue-authority <the-authority-to-queue-tasks> --crank-reward 1000000
```

The queue capacity is the maximum number of tasks that can be queued at once. Higher capacity means more tasks can be queued, but it also costs more rent in SOL.

### Closing a Task Queue

You can close a task queue by using the `task-queue close` command. This will refund the 1 SOL deposit and the funding amount.

First, close any queue authorities:

```bash
tuktuk -u <your-solana-url> task-queue remove-queue-authority --task-queue-name <your-queue-name> --queue-authority <your-wallet-address>
```

Then, close the task queue:

```bash
tuktuk -u <your-solana-url> task-queue close --task-queue-name <your-queue-name>
```

### Funding a Task Queue

Generally, tasks are funded by the wallet that creates the task. The only exception is for tasks that schedule more tasks.

If your task queue has any tasks that themselves queue tasks, you will need to keep it funded. This is because the task queue uses its own sol to fund recursively queued tasks. Note that you will not need to fund the task queue immediately if you specified a `funding-amount` in the `create` command.

```bash
tuktuk -u <your-solana-url> task-queue fund --task-queue-name <your-queue-name> --amount 100000000
```

### Adding Queue Authorities

Task queues are meant to be reused for multiple use cases. As such, there can be multiple wallets that have the authority to queue tasks. Note that this authority should not be given out blindly, as the authority can queue tasks that use up task queue funding, and can use the task queue's custom signers.

You can add queue authorities to a task queue by using the `add-queue-authority` command. Queue authorities can queue tasks on behalf of other users.

```bash
tuktuk -u <your-solana-url> task-queue add-queue-authority --task-queue-name <your-queue-name> --queue-authority <the-authority-to-queue-tasks>
```

An example use case for multiple authorities at Helium is that we have a program, hpl-crons, that allows users to
create specific jobs that automate helium tasks relating to things like their staked positions. Because we have audited these specific tasks, we allow a PDA signer of the hpl-crons program to queue tasks on behalf of users. Simultaneously, we also have an admin authority that can queue or remove tasks for the sake of troubleshooting.

### Queue a task

You can queue a task by using the `QueueTaskV0` instruction. There are many ways to call this function. You can do this via CPI in your smart contract, or you can use typescript. For examples of doing this in typescript, see the [typescript-examples](./typescript-examples) folder.

Similar functions are available in the tuktuk-sdk rust library. For an example of how to use this in a solana program, see the [cpi-example](./solana-programs/programs/cpi-example) and the corresponding [tests](./solana-programs/tests/tuktuk.ts).

### Remote Transactions

Sometimes transactions are complicated enough that you cannot compile it ahead of time. An example of this may be a transaction that uses cNFTs and requires a proof. In this case, you can run a remote server that returns the set of instructions. This server will need to sign the instructions so the program can trust that they are associated with the given task.

Tuktuk will `POST` to the remote URL with the following JSON body:

```json
{
  "task": "<task-pubkey>",
  "task_queue": "<task-queue-pubkey>",
  "task_queued_at": "<task-queued-at-timestamp>"
}
```

Your server will need to return the following JSON body:

```json
{
  "transaction": "<base64-encoded-transaction>",
  "remaining_accounts": "<base64-encoded-remaining-accounts>",
  "signature": "<base64-encoded-signature>"
}
```

You can see an example of this in the [remote-server-example](./solana-programs/packages/remote-example-server/src/index.ts).

You can queue such a task by using `remoteV0` instead of `compileV0` in the `QueueTaskV0` instruction.

```typescript
await program.methods.queueTaskV0({
  id: taskId,
  trigger: { now: {} },
  transaction: {
    remoteV0: {
      url: "http://localhost:3002/remote",
      signer: me,
    },
  },
});
```

### Monitoring the Task Queue

You can monitor tasks by using the cli:

```bash
tuktuk -u <your-solana-url> task list --task-queue-name <your-queue-name> --description <prefix>
```

The `--description` flag allows you to filter by prefix on the description field of tasks. This can be useful if you have a lot of tasks in a queue and want to only view specific kinds of tasks.

Note that this will only show you tasks that have not been run. Tasks that have been run are closed, with rent refunded to the task creator.

If a task is active but has not yet been run, the cli will display a simulation result for the task. This is to help you debug the task if for some reason it is not running.

### Cron Tasks

Sometimes, it's helpful to run a task on a specific schedule. You can do this by creating a cron job. A cron job will queue tasks onto a task queue at a specific time. The following example will queue a task every minute. Note that you will need to keep the cron funded so that it can, in turn, fund the task queue for each task it creates.

See an example of creating a cron job here [typescript-examples](./typescript-examples/src/cron-memo.ts).

### Monitoring the Cron Job

You can list your cron jobs by using the `cron list` command:

```bash
tuktuk -u <your-solana-url> cron list
```

You can get a particular cron job by name using the `cron get` command:

```bash
tuktuk -u <your-solana-url> cron get --cron-name <your-cron-job-name>
```

You can list the transactions in a cron job by using the `cron-transaction list` command:

```bash
tuktuk -u <your-solana-url> cron-transaction list --cron-name <your-cron-job-name>
```

You can delete a cron job by using the `cron close` command. First you must close all cron-transactions in the cron job (for-each id):

```bash
tuktuk -u <your-solana-url> cron-transaction close --cron-name <your-cron-job-name> --id <id>
```

Then you can close the cron job itself:

```bash
tuktuk -u <your-solana-url> cron close --cron-name <your-cron-job-name>
```

#### Cron Task Requeuing

Occasionally, a cron task can run out of funding and get removed from the queue. If this happens, the cron will have `removed_from_queue: true` set on its state. This can also happen when you forcefully remove it from the queue via `tuktuk task close`. In this case you should use the `cron requeue` command to requeue the task:

```bash
tuktuk -u <your-solana-url> cron requeue --cron-name <your-cron-job-name>
```

You can always check if your cron is queued by searching your task queue for `"queue <your-cron-job-name>"`:

```bash
tuktuk -u <your-solana-url> task list --task-queue-name <your-queue-name> --description "queue <your-cron-job-name>"
```

### Running a Task

Occasionally, a task could be missed by the tuktuk-crank-turner due to running out of retries. This can happen in cases where the task had a bug, which you later fixed. In this case, when you run `task list` you will see a successful simulation result for the task, but it will not have been run. If task list is taking too long to run, you can use the --skip-simulate flag.

You can run a task by using the `task run` command. This will run the task and mark it as run.

```bash
tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --task-id <task-id>
```

You can also run tasks by prefix using the `--description` flag. This can be useful if you have a lot of tasks in a queue and want to only run specific kinds of tasks.

```bash
tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --description <prefix>
```

### Closing Tasks

A task queue has a limited capacity. Therefore, you will want to close tasks that have failed and will never be able to succeed. When you close these tasks, you will be refunded the SOL fees. 

```bash
tuktuk -u <your-solana-url> task close --task-queue-name <your-queue-name> --task-id <task-id>
```

You can also close tasks by prefix using the `--description` flag. This can be useful if you have a lot of tasks in a queue and want to only close specific kinds of tasks.

```bash
tuktuk -u <your-solana-url> task close --task-queue-name <your-queue-name> --description <prefix>
```

## Development Setup

### Local Testing

1. In your Anchor.toml, make sure you have a test keypair defined:

```toml
[provider]
cluster = "localnet"
wallet = "~/.config/solana/id.json"  # Your local keypair. Find address using `solana address`
```

2. Install dependencies:

```bash
yarn install
```

3. Run the tests:

```bash
env TESTING=true anchor test
```

## Troubleshooting

### Common Issues and Solutions

#### Account Not Initialized Error
```
Program log: AnchorError caused by account: task. Error Code: AccountNotInitialized. Error Number: 3012. Error Message: The program expected this account to be already initialized.
```
This error is normal and can be safely ignored. It occurs when multiple crank turners attempt to execute the same task simultaneously. When the second crank turner tries to run the task, it fails because the first one already completed and closed the task account.

#### Task Not Running
If your task isn't being executed:

1. First, locate your task ID using the task list command:
   ```bash
   tuktuk -u <your-solana-url> task list --task-queue-name <your-queue-name>
   ```

2. If the task simulation looks successful but isn't running, try running it manually:
   ```bash
   tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --task-id <task-id>
   ```

3. To see the failed transaction in Solana Explorer (even for failing transactions), use the `--skip-preflight` flag:
   ```bash
   tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --task-id <task-id> --skip-preflight
   ```

#### Cron Job Stopped Running
If your cron job has stopped executing, it may have been removed from the queue due to insufficient funding or other issues. See the [Cron Task Requeuing](#cron-task-requeuing) section for instructions on how to requeue your cron job.

#### Task Fixed But Not Running
If you had a failing task that you've fixed, but crank turners are no longer attempting to run it, this is because crank turners ignore tasks after running out of retries. To resolve this:

1. Run the task manually using the task run command:
   ```bash
   tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --task-id <task-id>
   ```

2. If the task succeeds, it will be marked as complete and removed from the queue.

