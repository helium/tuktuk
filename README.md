# TukTuk

Run your permissionless cranks on Solana

![TukTuk](./tuktuk.jpg)

## Introduction

Tuktuk is a permissionless crank service. If you have a Solana smart contract endpoint that needs to be run on a trigger or specific time, you can use tuktuk to run it. Endponts need to be more or less permissionless, though you can have custom PDA signers provided by the tuktuk program.

Tuktuk's architecture allows for crankers to run a simple rust util that requires only a working solana RPC url and very minimal dependencies. There is no dependency on geyser, yellowstone, or any other indexing service.

Creators of Task Queues set their payment per-crank turn in $SOL. Crankers that run the tasks are paid out in $SOL for each crank they complete. There is a minimum deposit of 1 $SOL to create a task queue to discourage spam. This deposit is refunded when the task queue is closed. The intent is to minimize the number of task queues that crank turners need to watch. You should try to reuse task queues as much as possible. It is an antipattern to create a new task queue for each user, for example.

## Usage

Clone this repo and run `cargo build -p tuktuk-cli` to get the command line interface. It will be in the `target/debug/tuktuk` directory.

### Create a task queue

First, you'll need to get some $SOL to fund the task queue. You can get $SOL from [Jupiter Aggregator](https://www.jup.ag/swap/USDC-hntyVP6YFm1Hg25TN9WGLqM12b8TQmcknKrdu1oxWux).

Next, create a task queue. A task queue has a default crank reward that will be used for all tasks in the queue, but each task can override this reward. Since crankers pay sol (and possibly priority fees) for each crank, the crank reward should be higher than the cost of a crank or crankers will not be incentivized to run your task.

```
tuktuk task-queue -u <your-solana-url> create --name <your-queue-name> --capacity 10 --funding-amount 100000000 --queue-authority <the-authority-to-queue-tasks> --crank-reward 1000000
```

The queue capacity is the maximum number of tasks that can be queued at once. Higher capacity means more tasks can be queued, but it also costs more rent in $SOL.

### Fund a task queue

After tasks have been run, you will need to continually fund the task queue to keep it alive.

```
tuktuk task-queue -u <your-solana-url> fund --task-queue-name <your-queue-name> --amount 100000000
```

### Queue a task

You can queue a task by using the `QueueTaskV0` instruction. There are many ways to call this function. You can do this via CPI in your smart contract, or you can use typescript. Here is an example of a simple transfer of tokens from a TukTuk custom PDA at "test" to your wallet:

```typescript
import {
  init,
  compileTransaction,
  taskKey,
  customSignerKey,
  tuktukConfigKey
} from "@helium/tuktuk-sdk";

const program = await init(provider);
const taskQueue = taskQueueKey(tuktukConfigKey()[0], Buffer.from("my queue name"));
const taskId = 0;
const task = taskKey(taskQueue, taskId)[0];

// Create a PDA wallet associated with the task queue
const [wallet, bump] = customSignerKey(taskQueue, [Buffer.from("test")]);
// Create a testing mint
const mint = await createMint(provider, 0, me, me);
// Create an associated token account for the test PDA wallet
const lazySignerAta = await createAtaAndMint(provider, mint, 10, wallet);
const myAta = getAssociatedTokenAddressSync(mint, me);

// Transfer some tokens from PDA wallet to me via a task
const instructions: TransactionInstruction[] = [
  createAssociatedTokenAccountInstruction(wallet, myAta, me, mint),
  createTransferInstruction(lazySignerAta, myAta, wallet, 10),
];
// Compile the instructions and PDA into the args expected by the tuktuk program
const ({ transaction, remainingAccounts } = await compileTransaction(
        instructions,
        [[Buffer.from("test"), bumpBuffer]]
      ))
// Queue the task
await program.methods
  .queueTaskV0({
    id: taskId,
    // Example: 30 seconds from now
    // trigger: { timestamp: [new anchor.BN(Date.now() / 1000 + 30)] },
    // Example: run now
    trigger: { now: {} },
    transaction: {
      compiledV0: [transaction],
    },
    crankReward: null,
  })
  .remainingAccounts(remainingAccounts)
  .accounts({
    payer: me,
    taskQueue,
    task,
  })
  .rpc();
```

A similar compile_transaction function is available in the tuktuk-sdk rust library. For an example of how to use this in a solana program, see the [cpi-example](./solana-programs/programs/cpi-example) and the corresponding [tests](./solana-programs/tests/tuktuk.ts).

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

```
tuktuk -u <your-solana-url> task list --task-queue-name <your-queue-name>
```

Note that this will only show you tasks that have not been run. Tasks that have been run are closed, with rent refunded to the task creator.

If a task is active but has not yet been run, the cli will display a simulation result for the task. This is to help you debug the task if for some reason it is not running.

### Cron Tasks

Sometimes, it's helpful to run a task on a specific schedule. You can do this by creating a cron job. A cron job will queue tasks onto a task queue at a specific time. The following example will queue a task every minute. Note that you will need to keep the cron funded so that it can, in turn, fund the task queue for each task it creates.

```
tuktuk -u <your-solana-url> cron create --name <your-cron-job-name> --task-queue-name <your-queue-name>  --schedule "0 * * * * *" --free-tasks-per-transaction 0 --funding-amount 1000000000
```

A single cron job can queue multiple transactions. You can add transactions to a cron job by using the `cron-transaction` command. To add a normal transaction to a cron job, it is easier to write a script:

```typescript
import {
  compileTransaction,
  taskKey,
  customSignerKey,
  tuktukConfigKey
} from "@helium/tuktuk-sdk";
import { init as initCron, cronJobKey } from "@helium/cron-sdk";

const cronProgram = await initCron(provider);
const cronJob = cronJobKey(provider.wallet.publicKey, 0)[0]
const taskQueue = taskQueueKey(tuktukConfigKey()[0], Buffer.from("my queue name"));
const taskId = 0;
const task = taskKey(taskQueue, taskId)[0];

// Create a PDA wallet associated with the task queue
const [wallet, bump] = customSignerKey(taskQueue, [Buffer.from("test")]);
const instructions: TransactionInstruction[] = [
  new TransactionInstruction({
    keys: [{ pubkey: wallet, isSigner: true, isWritable: true }],
    data: Buffer.from("I'm a remote transaction!", "utf-8"),
    programId: new PublicKey(
      "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"
    ),
  }),
];

// Compile the instructions and PDA into the args expected by the tuktuk program
const ({ transaction, remainingAccounts } = await compileTransaction(
  instructions,
  [[Buffer.from("test"), bumpBuffer]]
));

await cronProgram.methods
  .addCronTransactionV0({
    index: 0,
    transactionSource: {
      compiledV0: [transaction],
    },
  })
  .accounts({
    payer: me,
    cronJob,
    cronJobTransaction: cronJobTransactionKey(cronJob, 0)[0],
  })
  .remainingAccounts(remainingAccounts)
  .rpc({ skipPreflight: true });
```

To add a remote transaction, you can use the `create-remote` command:

```
tuktuk -u <your-solana-url> cron-transaction create-remote --url http://localhost:3002/remote --signer $(solana address) --id 0 --cron-name Noah
```

### Monitoring the Cron Job

You can list your cron jobs by using the `cron list` command:

```
tuktuk -u <your-solana-url> cron list
```

You can get a particular cron job by name using the `cron get` command:

```
tuktuk -u <your-solana-url> cron get --cron-name <your-cron-job-name>
```

You can list the transactions in a cron job by using the `cron-transaction list` command:

```
tuktuk -u <your-solana-url> cron-transaction list --cron-name <your-cron-job-name>
```

You can delete a cron job by using the `cron close` command. First it is recommended that you close all cron-transactions in the cron job (for-each id):

```
tuktuk -u <your-solana-url> cron-transaction close --cron-name <your-cron-job-name> --id <id>
```

Then you can close the cron job itself:

```
tuktuk -u <your-solana-url> cron close --cron-name <your-cron-job-name>
```
