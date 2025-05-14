import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { cronJobKey, cronJobNameMappingKey, cronJobTransactionKey, init as initCron, userCronJobsKey, createCronJob } from "@helium/cron-sdk";
import {
  createAtaAndMint,
  createMint,
  populateMissingDraftInfo,
  sendAndConfirmWithRetry,
  sendInstructions,
  toVersionedTx,
  withPriorityFees,
} from "@helium/spl-utils";
import {
  CompiledTransactionV0,
  compileTransaction,
  customSignerKey,
  init as initTuktuk,
  nextAvailableTaskIds,
  runTask,
  taskKey,
  taskQueueKey,
  taskQueueNameMappingKey,
  tuktukConfigKey,
} from "@helium/tuktuk-sdk";
import {
  createAssociatedTokenAccountIdempotentInstruction,
  createTransferInstruction,
  getAssociatedTokenAddressSync
} from "@solana/spl-token";
import {
  AccountMeta,
  ComputeBudgetProgram,
  Keypair,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
} from "@solana/web3.js";
import chai from "chai";
import { Cron } from "../target/types/cron";
import { Tuktuk } from "../target/types/tuktuk";
import { ensureIdls, makeid } from "./utils";
const { expect } = chai;

describe("cron", () => {
  // Configure the client to use the local cluster.
  anchor.setProvider(anchor.AnchorProvider.local("http://127.0.0.1:8899"));

  let tuktukProgram: Program<Tuktuk>;
  let cronProgram: Program<Cron>;
  const provider = anchor.getProvider() as anchor.AnchorProvider;
  const me = provider.wallet.publicKey;
  const tuktukConfig = tuktukConfigKey()[0];

  before(async () => {
    await ensureIdls();
    tuktukProgram = await initTuktuk(provider);
    cronProgram = await initCron(provider);
  });

  describe("with a task queue", () => {
    let name: string;
    let taskQueue: PublicKey;
    let transaction: CompiledTransactionV0;
    let remainingAccounts: AccountMeta[];
    const crankReward: anchor.BN = new anchor.BN(1000000000);

    beforeEach(async () => {
      name = makeid(10);
      if (
        !(await tuktukProgram.account.tuktukConfigV0.fetchNullable(
          tuktukConfig
        ))
      ) {
        await tuktukProgram.methods
          .initializeTuktukConfigV0({
            minDeposit: new anchor.BN(100000000),
          })
          .accounts({
            authority: me,
          })
          .rpc();
      }
      const config = await tuktukProgram.account.tuktukConfigV0.fetch(
        tuktukConfig
      );
      const nextTaskQueueId = config.nextTaskQueueId;
      taskQueue = taskQueueKey(tuktukConfig, nextTaskQueueId)[0];
      await tuktukProgram.methods
        .initializeTaskQueueV0({
          name,
          minCrankReward: crankReward,
          capacity: 100,
          lookupTables: [],
          staleTaskAge: 10000,
        })
        .accounts({
          tuktukConfig,
          payer: me,
          updateAuthority: me,
          taskQueue,
          taskQueueNameMapping: taskQueueNameMappingKey(tuktukConfig, name)[0],
        })
        .rpc();

      await tuktukProgram.methods
        .addQueueAuthorityV0()
        .accounts({
          payer: me,
          queueAuthority: me,
          taskQueue,
        })
        .rpc();

      const [wallet, bump] = customSignerKey(taskQueue, [Buffer.from("test")]);
      await sendInstructions(provider, [
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: wallet,
          lamports: 1000000000,
        }),
      ]);
      const mint = await createMint(provider, 0, me, me);
      const lazySignerAta = await createAtaAndMint(provider, mint, 10000, wallet);
      const myAta = getAssociatedTokenAddressSync(mint, me);

      // Transfer some tokens from lazy signer to me
      const instructions: TransactionInstruction[] = [
        createAssociatedTokenAccountIdempotentInstruction(wallet, myAta, me, mint),
        createTransferInstruction(lazySignerAta, myAta, wallet, 10),
      ];

      const bumpBuffer = Buffer.alloc(1);
      bumpBuffer.writeUint8(bump);
      ({ transaction, remainingAccounts } = await compileTransaction(
        instructions,
        [[Buffer.from("test"), bumpBuffer]]
      ));
    });

    it("initializes a cron job and runs the task on a schedule", async () => {
      const name = makeid(10);
      let userCronJobs = userCronJobsKey(me)[0];
      const userCronJobsAcc = await cronProgram.account.userCronJobsV0.fetchNullable(userCronJobs);
      const crankTurner = Keypair.generate();
      const task = taskKey(taskQueue, 0)[0];
      const cronJob = cronJobKey(me, userCronJobsAcc?.nextCronJobId ?? 0)[0];
      const cronJobNameMapping = cronJobNameMappingKey(me, name)[0]

      // Fund accounts
      await sendInstructions(provider, [
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: crankTurner.publicKey,
          lamports: 10000000000,
        }),
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: taskQueue,
          lamports: 1000000000,
        }),
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: cronJob,
          lamports: 10000000000,
        }),
      ]);

      // Initialize cron job
      await cronProgram.methods
        .initializeCronJobV0({
          name,
          schedule: "*/1 * * * * *", // Run every second
          freeTasksPerTransaction: 5,
          numTasksPerQueueCall: 1,
        })
        .preInstructions([
          ComputeBudgetProgram.setComputeUnitLimit({
            units: 1000000,
          }),
        ])
        .accounts({
          payer: me,
          authority: me,
          cronJobNameMapping,
          taskQueue,
          cronJob,
          task,
        })
        .rpc({ skipPreflight: true });

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

      // Run the initial task that queues the cron tasks
      const ixs = await runTask({
        program: tuktukProgram,
        task,
        crankTurner: crankTurner.publicKey,
      });

      const tx = toVersionedTx(
        await populateMissingDraftInfo(provider.connection, {
          feePayer: crankTurner.publicKey,
          instructions: await withPriorityFees({
            instructions: ixs,
            connection: provider.connection,
            computeUnits: 1000000,
          })
        })
      );

      await tx.sign([crankTurner]);

      await sendAndConfirmWithRetry(
        provider.connection,
        Buffer.from(tx.serialize()),
        {
          skipPreflight: true,
          maxRetries: 0,
        },
        "confirmed"
      );

      // Wait for next scheduled execution
      await sleep(2000);

      // Run the scheduled task
      const task2 = taskKey(taskQueue, 1)[0];
      const task3 = taskKey(taskQueue, 2)[0];
      const taskQueueAcc = await tuktukProgram.account.taskQueueV0.fetch(taskQueue);
      const task2Acc = await tuktukProgram.account.taskV0.fetch(task2);
      const task3Acc = await tuktukProgram.account.taskV0.fetch(task3);
      const nextAvailable = nextAvailableTaskIds(taskQueueAcc.taskBitmap, task2Acc.freeTasks + task3Acc.freeTasks);
      const ixs2 = await runTask({
        program: tuktukProgram,
        task: task2,
        crankTurner: crankTurner.publicKey,
        nextAvailableTaskIds: nextAvailable.slice(0, task2Acc.freeTasks),
      });

      const ixs3 = await runTask({
        program: tuktukProgram,
        task: task3,
        crankTurner: crankTurner.publicKey,
        nextAvailableTaskIds: nextAvailable.slice(task2Acc.freeTasks, task2Acc.freeTasks + task3Acc.freeTasks),
      });
      const tx2 = toVersionedTx(
        await populateMissingDraftInfo(provider.connection, {
          feePayer: crankTurner.publicKey,
          instructions: await withPriorityFees({
            instructions: [...ixs2, ...ixs3],
            connection: provider.connection,
            computeUnits: 1000000,
          })
        })
      );
      await tx2.sign([crankTurner]);
      console.log(await sendAndConfirmWithRetry(
        provider.connection,
        Buffer.from(tx2.serialize()),
        {
          skipPreflight: true,
          maxRetries: 0,
        },
        "confirmed"
      ));

      const cronJobV0 = await cronProgram.account.cronJobV0.fetch(cronJob);
      const nextScheduleTask = await tuktukProgram.account.taskV0.fetchNullable(cronJobV0.nextScheduleTask);
      expect(nextScheduleTask).to.not.be.null;
    });
  });
});

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
