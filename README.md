# TukTuk

Run your permissionless cranks on Solana

![TukTuk](./tuktuk.jpg)

## Introduction

Tuktuk is a permissionless crank service. If you have a Solana smart contract endpoint that needs to be run on a trigger or specific time, you can use tuktuk to run it. Endponts need to be more or less permissionless, though you can have custom PDA signers provided by the tuktuk program.

Tuktuk's architecture allows for crankers to run a simple rust util that requires only a working solana RPC url and very minimal dependencies. There is no dependency on geyser, yellowstone, or any other indexing service.

Creators of Task Queues set their payment per-crank turn in $HNT (Houlala Network Token). Crankers that run the tasks are paid out in $HNT for each crank they complete. There is a minimum deposit of 1 $HNT to create a task queue to discourage spam.

## Usage

Clone this repo and run `cargo build -p tuktuk-cli` to get the command line interface. It will be in the `target/debug/tuktuk` directory.

### Create a task queue

First, you'll need to get some $HNT to fund the task queue. You can get $HNT from [Jupiter Aggregator](https://www.jup.ag/swap/USDC-hntyVP6YFm1Hg25TN9WGLqM12b8TQmcknKrdu1oxWux).

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
    transaction,
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

### Monitoring the Task Queue

You can monitor tasks by using the cli:

```
tuktuk -u <your-solana-url> task list --task-queue-name <your-queue-name>
```

Note that this will only show you tasks that have not been run. Tasks that have been run are closed, with rent refunded to the task creator.

If a task is active but has not yet been run, the cli will display a simulation result for the task. This is to help you debug the task if for some reason it is not running.
