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
import { ensureIdls, makeid, readyTasks } from "./utils";
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

      // Run the scheduled tasks. A turner takes its free task ids from a randomized start so
      // concurrent turners do not collide, so read back which tasks are due rather than
      // assuming where they landed.
      const taskQueueAcc = await tuktukProgram.account.taskQueueV0.fetch(taskQueue);
      const due = await readyTasks(tuktukProgram, taskQueue, 2);
      expect(due).to.have.length(2);
      const [task2, task3] = due;
      const task2Acc = await tuktukProgram.account.taskV0.fetch(task2);
      const task3Acc = await tuktukProgram.account.taskV0.fetch(task3);
      const nextAvailable = nextAvailableTaskIds(
        taskQueueAcc.taskBitmap,
        task2Acc.freeTasks + task3Acc.freeTasks,
        true,
        taskQueueAcc.capacity
      );
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

    describe("with a cron job", () => {
      const numTasksPerQueueCall = 1;
      // Every test gets a fresh task queue, so the job's own task lands on id 0.
      const initialTaskId = 0;
      let cronJob: PublicKey;
      let cronName: string;

      const returnAccount = (n: number, job: PublicKey) =>
        PublicKey.findProgramAddressSync(
          [Buffer.from(`task_return_account_${n}`), job.toBuffer()],
          cronProgram.programId
        )[0];

      /** Every account an instruction on this cron job is allowed to touch. */
      const watched = (job: PublicKey) => [
        job,
        returnAccount(1, job),
        returnAccount(2, job),
      ];

      async function snapshot(keys: PublicKey[]) {
        const infos = await provider.connection.getMultipleAccountsInfo(keys);
        return infos.map((i) => ({
          lamports: i?.lamports ?? 0,
          owner: i?.owner?.toBase58() ?? null,
          data: i?.data ?? Buffer.alloc(0),
        }));
      }

      function expectUnchanged(before: any[], after: any[], keys: PublicKey[]) {
        keys.forEach((k, i) => {
          const at = `${k.toBase58()}`;
          expect(after[i].lamports, `${at} lamports`).to.eq(before[i].lamports);
          // `write_return_tasks` assigns a system-owned return account to the cron program
          // before writing it, and that reassignment outlives a realloc back down.
          expect(after[i].owner, `${at} owner`).to.eq(before[i].owner);
          expect(after[i].data.equals(before[i].data), `${at} data`).to.be.true;
        });
      }

      async function run(task: PublicKey, freeTaskIds: number[]) {
        await sendInstructions(provider, [
          ComputeBudgetProgram.setComputeUnitLimit({ units: 1000000 }),
          ...(await runTask({
            program: tuktukProgram,
            task,
            crankTurner: me,
            nextAvailableTaskIds: freeTaskIds,
          })),
        ]);
      }

      /**
       * Assert a refusal, attributed to a program and to a named error.
       *
       * The error number alone does not identify anything here: cron and tuktuk both number
       * custom errors from 6000, so cron's `TaskAlreadyQueued` (6008) and `NotEnoughAccounts`
       * (6009) are tuktuk's `InvalidTaskPDA` and `TaskQueueInsufficientFunds`, and both programs
       * run in these transactions. Simulating yields the logs whatever the send path is, and the
       * logs carry the failing program and the error's name.
       *
       * Simulation reports `is_signer` from the message header rather than from a verified
       * signature, so an account the client marks as a signer reaches the program as one. That
       * is what lets the seeds constraint be tested apart from the `Signer` type.
       */
      async function expectRefusal(
        ixs: TransactionInstruction[],
        opts: { program: PublicKey; error: string; code: number; account?: string }
      ) {
        const tx = toVersionedTx(
          await populateMissingDraftInfo(provider.connection, {
            feePayer: me,
            instructions: ixs,
          })
        );
        const sim = await provider.connection.simulateTransaction(tx, {
          sigVerify: false,
          replaceRecentBlockhash: true,
        });
        const logs = (sim.value.logs ?? []).join("\n");
        expect(sim.value.err, `expected a failure, got success:\n${logs}`).to.not
          .be.null;
        expect(logs, "failing program").to.include(
          `Program ${opts.program.toBase58()} failed`
        );
        expect(logs, "error name and number").to.include(
          `Error Code: ${opts.error}. Error Number: ${opts.code}.`
        );
        if (opts.account) {
          expect(logs, "offending account").to.include(
            `AnchorError caused by account: ${opts.account}.`
          );
        }
      }

      /**
       * The account list a schedule task carried before v1 existed, written out rather than
       * regenerated from the IDL. Rebuilding it from `cronProgram.methods` would track a change
       * to `QueueCronTasksV0` instead of catching one, which is the whole point of the fixture.
       */
      function legacyScheduleIx(job: PublicKey) {
        return new TransactionInstruction({
          programId: cronProgram.programId,
          keys: [
            { pubkey: job, isSigner: false, isWritable: true },
            { pubkey: taskQueue, isSigner: false, isWritable: false },
            { pubkey: returnAccount(1, job), isSigner: false, isWritable: true },
            { pubkey: returnAccount(2, job), isSigner: false, isWritable: true },
            {
              pubkey: SystemProgram.programId,
              isSigner: false,
              isWritable: false,
            },
            {
              pubkey: cronJobTransactionKey(job, 0)[0],
              isSigner: false,
              isWritable: false,
            },
          ],
          data: cronProgram.coder.instruction.encode("queueCronTasksV0", {}),
        });
      }

      function isV1Schedule(task: any) {
        const compiled = task.transaction.compiledV0![0];
        expect(compiled.instructions).to.have.length(1);
        expect(
          Buffer.from(compiled.instructions[0].data)
            .subarray(0, 8)
            .equals(
              cronProgram.coder.instruction
                .encode("queueCronTasksV1", {})
                .subarray(0, 8)
            ),
          "schedule task calls queue_cron_tasks_v1"
        ).to.be.true;
        expect(compiled.signerSeeds, "carries the cron signer seeds").to.have.length(1);
        expect(task.freeTasks).to.eq(numTasksPerQueueCall + 1);
      }

      async function createCronJobFor(
        name: string,
        lamports: number,
        perQueueCall: number = numTasksPerQueueCall
      ) {
        const userCronJobsAcc =
          await cronProgram.account.userCronJobsV0.fetchNullable(
            userCronJobsKey(me)[0]
          );
        const job = cronJobKey(me, userCronJobsAcc?.nextCronJobId ?? 0)[0];
        if (lamports > 0) {
          await sendInstructions(provider, [
            SystemProgram.transfer({
              fromPubkey: me,
              toPubkey: job,
              lamports,
            }),
          ]);
        }
        const queueAcc = await tuktukProgram.account.taskQueueV0.fetch(taskQueue);
        const taskId = nextAvailableTaskIds(
          queueAcc.taskBitmap,
          1,
          false,
          queueAcc.capacity
        )[0];
        await cronProgram.methods
          .initializeCronJobV0({
            name,
            schedule: "*/1 * * * * *",
            freeTasksPerTransaction: 5,
            numTasksPerQueueCall: perQueueCall,
          })
          .preInstructions([
            ComputeBudgetProgram.setComputeUnitLimit({ units: 1000000 }),
          ])
          .accounts({
            payer: me,
            authority: me,
            cronJobNameMapping: cronJobNameMappingKey(me, name)[0],
            taskQueue,
            cronJob: job,
            task: taskKey(taskQueue, taskId)[0],
          })
          .rpc({ skipPreflight: true });
        return { job, taskId };
      }

      async function addCronTransaction(job: PublicKey, index: number) {
        await cronProgram.methods
          .addCronTransactionV0({
            index,
            transactionSource: { compiledV0: [transaction] },
          })
          .accounts({
            payer: me,
            cronJob: job,
            cronJobTransaction: cronJobTransactionKey(job, index)[0],
          })
          .remainingAccounts(remainingAccounts)
          .rpc({ skipPreflight: true });
      }

      function requeueIx(job: PublicKey, recorded: PublicKey, taskId: number) {
        return cronProgram.methods
          .requeueCronTaskV1({ taskId })
          .accounts({
            payer: me,
            queueAuthority: me,
            cronJob: job,
            nextScheduleTask: recorded,
            taskQueue,
            task: taskKey(taskQueue, taskId)[0],
          })
          // Requeue compiles a schedule transaction and queues it through tuktuk, which costs
          // more than the 200k an instruction gets by default. The CLI, which is what sends this
          // instruction, derives its limit by simulating first.
          .preInstructions([
            ComputeBudgetProgram.setComputeUnitLimit({ units: 1000000 }),
          ]);
      }

      async function dequeue(task: PublicKey) {
        const acc = await tuktukProgram.account.taskV0.fetch(task);
        await tuktukProgram.methods
          .dequeueTaskV0()
          .accountsPartial({
            queueAuthority: me,
            rentRefund: acc.rentRefund,
            taskQueue,
            task,
          })
          .rpc();
      }

      beforeEach(async () => {
        // Well clear of tuktuk's own insufficient-funds floor, so no test can pass on that
        // error instead of the one it names.
        await sendInstructions(provider, [
          SystemProgram.transfer({
            fromPubkey: me,
            toPubkey: taskQueue,
            lamports: 20000000000,
          }),
        ]);
        cronName = makeid(10);
        ({ job: cronJob } = await createCronJobFor(cronName, 10000000000));
        await addCronTransaction(cronJob, 0);
      });

      it("hands a schedule task compiled against queue_cron_tasks_v0 over to v1", async () => {
        const legacyIx = legacyScheduleIx(cronJob);
        // The account list `queue_cron_tasks_v0` presents is frozen: every schedule task
        // already queued names it, so a change here strands all of them.
        const current = await cronProgram.methods
          .queueCronTasksV0()
          .accounts({ cronJob, taskQueue })
          .remainingAccounts([
            {
              pubkey: cronJobTransactionKey(cronJob, 0)[0],
              isSigner: false,
              isWritable: false,
            },
          ])
          .instruction();
        expect(
          current.keys.map((k) => [
            k.pubkey.toBase58(),
            k.isWritable,
            k.isSigner,
          ]),
          "stored schedule shape still matches the program"
        ).to.deep.eq(
          legacyIx.keys.map((k) => [k.pubkey.toBase58(), k.isWritable, k.isSigner])
        );

        // In production the record names the legacy task itself, which `run_task_v0` closes in
        // the run that invokes the handover. Retiring the job's own task reaches the same state
        // the successor is measured against: the record names an account holding nothing.
        await dequeue(taskKey(taskQueue, initialTaskId)[0]);

        const legacy = compileTransaction([legacyIx], []);
        const legacyTaskId = 1;
        const legacyTask = taskKey(taskQueue, legacyTaskId)[0];
        await tuktukProgram.methods
          .queueTaskV0({
            id: legacyTaskId,
            trigger: { now: {} },
            transaction: { compiledV0: [legacy.transaction] },
            crankReward: null,
            freeTasks: numTasksPerQueueCall + 1,
            description: "queue legacy",
          })
          .remainingAccounts(legacy.remainingAccounts)
          .accounts({ payer: me, taskQueue, task: legacyTask })
          .rpc({ skipPreflight: true });

        const keys = watched(cronJob);
        const before = await snapshot(keys);
        const successorIds = [20, 21];
        await run(legacyTask, successorIds);
        // The handover records nothing, on any account it is handed.
        expectUnchanged(before, await snapshot(keys), keys);

        const successor = taskKey(taskQueue, successorIds[0])[0];
        const successorAcc = await tuktukProgram.account.taskV0.fetch(successor);
        expect(successorAcc.trigger.now, "successor runs immediately").to.not.be
          .undefined;
        isV1Schedule(successorAcc);
        expect(
          await tuktukProgram.account.taskV0.fetchNullable(
            taskKey(taskQueue, successorIds[1])[0]
          ),
          "only one free task is taken"
        ).to.be.null;

        // The successor does the work the handover left, and pays the queue for it.
        const jobBefore = await provider.connection.getAccountInfo(cronJob);
        const nextIds = [30, 31];
        await run(successor, nextIds);
        const jobAfter = await provider.connection.getAccountInfo(cronJob);
        // One schedule task plus one cron transaction, at min_crank_reward each. The queue's
        // own net is not assertable by direction: it receives this payment and then funds the
        // rent and reward of the tasks it creates from the same balance.
        expect(
          jobBefore!.lamports - jobAfter!.lamports,
          "cron job pays the queue for the tasks it queued"
        ).to.eq(crankReward.toNumber() * 2);

        const cronJobAcc = await cronProgram.account.cronJobV0.fetch(cronJob);
        expect(cronJobAcc.nextScheduleTask.toBase58()).to.eq(
          taskKey(taskQueue, nextIds[0])[0].toBase58()
        );
        expect(cronJobAcc.removedFromQueue).to.be.false;
        expect(
          await tuktukProgram.account.taskV0.fetchNullable(
            taskKey(taskQueue, nextIds[1])[0]
          ),
          "the period's transaction was queued"
        ).to.not.be.null;
      });

      it("hands over once the task the record names is gone again", async () => {
        // The id the record names is free from the moment the legacy task closes, and the
        // successor runs a little later, so another task on this queue can hold that address
        // when the successor arrives. The handover waits for it rather than consuming it.
        await dequeue(taskKey(taskQueue, initialTaskId)[0]);

        const legacy = compileTransaction([legacyScheduleIx(cronJob)], []);
        const legacyTaskId = 1;
        const legacyTask = taskKey(taskQueue, legacyTaskId)[0];
        await tuktukProgram.methods
          .queueTaskV0({
            id: legacyTaskId,
            trigger: { now: {} },
            transaction: { compiledV0: [legacy.transaction] },
            crankReward: null,
            freeTasks: numTasksPerQueueCall + 1,
            description: "queue legacy",
          })
          .remainingAccounts(legacy.remainingAccounts)
          .accounts({ payer: me, taskQueue, task: legacyTask })
          .rpc({ skipPreflight: true });

        const successorIds = [20, 21];
        await run(legacyTask, successorIds);
        const successor = taskKey(taskQueue, successorIds[0])[0];

        // Someone else's task, holding the address the record names. It carries no
        // instructions, so running it is the whole of what it does.
        const squatter = compileTransaction([], []);
        await tuktukProgram.methods
          .queueTaskV0({
            id: initialTaskId,
            trigger: { now: {} },
            transaction: { compiledV0: [squatter.transaction] },
            crankReward: null,
            freeTasks: 0,
            description: "unrelated",
          })
          .remainingAccounts(squatter.remainingAccounts)
          .accounts({ payer: me, taskQueue, task: taskKey(taskQueue, initialTaskId)[0] })
          .rpc({ skipPreflight: true });

        const nextIds = [30, 31];
        await expectRefusal(
          await runTask({
            program: tuktukProgram,
            task: successor,
            crankTurner: me,
            nextAvailableTaskIds: nextIds,
          }),
          {
            program: cronProgram.programId,
            error: "WrongScheduleTask",
            code: 6012,
          }
        );

        // The successor is still queued: the refusal fails the run rather than consuming it.
        expect(
          await tuktukProgram.account.taskV0.fetchNullable(successor),
          "the successor survives a refused run"
        ).to.not.be.null;

        // The other task runs and closes like any other, and the handover then completes.
        await run(taskKey(taskQueue, initialTaskId)[0], []);
        await run(successor, nextIds);

        const cronJobAcc = await cronProgram.account.cronJobV0.fetch(cronJob);
        expect(
          cronJobAcc.nextScheduleTask.toBase58(),
          "the chain carries on from the successor"
        ).to.eq(taskKey(taskQueue, nextIds[0])[0].toBase58());
        expect(cronJobAcc.removedFromQueue).to.be.false;
      });

      it("leaves every account untouched when queue_cron_tasks_v0 is called directly", async () => {
        const keys = watched(cronJob);
        const before = await snapshot(keys);
        await cronProgram.methods
          .queueCronTasksV0()
          .accounts({ cronJob, taskQueue })
          .rpc({ skipPreflight: true });
        expectUnchanged(before, await snapshot(keys), keys);
      });

      async function recordedTask(job: PublicKey) {
        return (await cronProgram.account.cronJobV0.fetch(job)).nextScheduleTask;
      }

      it("refuses queue_cron_tasks_v1 without the cron signer", async () => {
        const recorded = await recordedTask(cronJob);
        const cronSigner = customSignerKey(taskQueue, [
          Buffer.from("cron"),
          cronJob.toBuffer(),
        ])[0];
        const ix = await cronProgram.methods
          .queueCronTasksV1()
          .accountsPartial({ cronJob, taskQueue, cronSigner, recordedScheduleTask: recorded })
          .remainingAccounts([
            {
              pubkey: cronJobTransactionKey(cronJob, 0)[0],
              isSigner: false,
              isWritable: false,
            },
            { pubkey: taskKey(taskQueue, 40)[0], isSigner: false, isWritable: true },
          ])
          .instruction();
        expect(ix.keys.some((k) => k.pubkey.equals(cronSigner) && k.isSigner)).to
          .be.true;
        ix.keys = ix.keys.map((k) =>
          k.pubkey.equals(cronSigner) ? { ...k, isSigner: false } : k
        );
        await expectRefusal([ix], {
          program: cronProgram.programId,
          error: "AccountNotSigner",
          code: 3010,
          account: "cron_signer",
        });
      });

      it("refuses queue_cron_tasks_v1 signed by an address that is not the cron signer", async () => {
        const recorded = await recordedTask(cronJob);
        const impostor = Keypair.generate();
        const ix = await cronProgram.methods
          .queueCronTasksV1()
          .accountsPartial({ cronJob, taskQueue, cronSigner: impostor.publicKey, recordedScheduleTask: recorded })
          .remainingAccounts([
            {
              pubkey: cronJobTransactionKey(cronJob, 0)[0],
              isSigner: false,
              isWritable: false,
            },
            { pubkey: taskKey(taskQueue, 40)[0], isSigner: false, isWritable: true },
          ])
          .instruction();
        await expectRefusal([ix], {
          program: cronProgram.programId,
          error: "ConstraintSeeds",
          code: 2006,
          account: "cron_signer",
        });
      });

      it("refuses queue_cron_tasks_v1 handed something other than the instructions sysvar", async () => {
        // The running task is read out of that account, so it has to be the real sysvar.
        const recorded = await recordedTask(cronJob);
        const cronSigner = customSignerKey(taskQueue, [
          Buffer.from("cron"),
          cronJob.toBuffer(),
        ])[0];
        const ix = await cronProgram.methods
          .queueCronTasksV1()
          .accountsPartial({
            cronJob,
            taskQueue,
            cronSigner,
            recordedScheduleTask: recorded,
            sysvarInstructions: taskKey(taskQueue, 41)[0],
          })
          .remainingAccounts([
            {
              pubkey: cronJobTransactionKey(cronJob, 0)[0],
              isSigner: false,
              isWritable: false,
            },
            { pubkey: taskKey(taskQueue, 40)[0], isSigner: false, isWritable: true },
          ])
          .instruction();
        await expectRefusal([ix], {
          program: cronProgram.programId,
          error: "ConstraintAddress",
          code: 2012,
          account: "sysvar_instructions",
        });
      });

      it("refuses a schedule run that names a record the cron job does not hold", async () => {
        // The record a schedule run is measured against is the cron job's own field, not an
        // account the run names.
        const [cronSigner, bump] = customSignerKey(taskQueue, [
          Buffer.from("cron"),
          cronJob.toBuffer(),
        ]);
        const decoy = taskKey(taskQueue, 61)[0];
        expect(
          await tuktukProgram.account.taskV0.fetchNullable(decoy),
          "the decoy holds nothing, so the vacancy arm would admit it"
        ).to.be.null;
        const ix = await cronProgram.methods
          .queueCronTasksV1()
          .accountsPartial({
            cronJob,
            taskQueue,
            cronSigner,
            recordedScheduleTask: decoy,
          })
          .remainingAccounts([
            {
              pubkey: cronJobTransactionKey(cronJob, 0)[0],
              isSigner: false,
              isWritable: false,
            },
          ])
          .instruction();
        const bumpBuffer = Buffer.alloc(1);
        bumpBuffer.writeUint8(bump);
        const compiled = compileTransaction(
          [ix],
          [[Buffer.from("cron"), cronJob.toBuffer(), bumpBuffer]]
        );
        const mintTask = taskKey(taskQueue, 6)[0];
        await tuktukProgram.methods
          .queueTaskV0({
            id: 6,
            trigger: { now: {} },
            transaction: { compiledV0: [compiled.transaction] },
            crankReward: null,
            freeTasks: numTasksPerQueueCall + 1,
            description: "queue decoy",
          })
          .remainingAccounts(compiled.remainingAccounts)
          .accounts({ payer: me, taskQueue, task: mintTask })
          .rpc({ skipPreflight: true });

        const keys = watched(cronJob);
        const before = await snapshot(keys);
        const ixs = await runTask({
          program: tuktukProgram,
          task: mintTask,
          crankTurner: me,
          nextAvailableTaskIds: [84, 85],
        });
        await expectRefusal(
          [ComputeBudgetProgram.setComputeUnitLimit({ units: 1000000 }), ...ixs],
          {
            program: cronProgram.programId,
            error: "WrongScheduleTask",
            code: 6012,
          }
        );
        expectUnchanged(before, await snapshot(keys), keys);
      });

      it("refuses a schedule run against a task queue the cron job does not name", async () => {
        const recorded = await recordedTask(cronJob);
        const otherName = makeid(10);
        const config = await tuktukProgram.account.tuktukConfigV0.fetch(tuktukConfig);
        const otherQueue = taskQueueKey(tuktukConfig, config.nextTaskQueueId)[0];
        await tuktukProgram.methods
          .initializeTaskQueueV0({
            name: otherName,
            minCrankReward: crankReward,
            capacity: 100,
            lookupTables: [],
            staleTaskAge: 10000,
          })
          .accounts({
            tuktukConfig,
            payer: me,
            updateAuthority: me,
            taskQueue: otherQueue,
            taskQueueNameMapping: taskQueueNameMappingKey(tuktukConfig, otherName)[0],
          })
          .rpc();
        await tuktukProgram.methods
          .addQueueAuthorityV0()
          .accounts({ payer: me, queueAuthority: me, taskQueue: otherQueue })
          .rpc();
        await sendInstructions(provider, [
          SystemProgram.transfer({
            fromPubkey: me,
            toPubkey: otherQueue,
            lamports: 20000000000,
          }),
        ]);

        const [otherSigner, otherBump] = customSignerKey(otherQueue, [
          Buffer.from("cron"),
          cronJob.toBuffer(),
        ]);
        const ix = await cronProgram.methods
          .queueCronTasksV1()
          .accountsPartial({ cronJob, taskQueue: otherQueue, cronSigner: otherSigner, recordedScheduleTask: recorded })
          .remainingAccounts([
            {
              pubkey: cronJobTransactionKey(cronJob, 0)[0],
              isSigner: false,
              isWritable: false,
            },
          ])
          .instruction();
        const bumpBuffer = Buffer.alloc(1);
        bumpBuffer.writeUint8(otherBump);
        const compiled = compileTransaction(
          [ix],
          [[Buffer.from("cron"), cronJob.toBuffer(), bumpBuffer]]
        );

        const otherTaskId = 3;
        const otherTask = taskKey(otherQueue, otherTaskId)[0];
        await tuktukProgram.methods
          .queueTaskV0({
            id: otherTaskId,
            trigger: { now: {} },
            transaction: { compiledV0: [compiled.transaction] },
            crankReward: null,
            freeTasks: numTasksPerQueueCall + 1,
            description: "queue other",
          })
          .remainingAccounts(compiled.remainingAccounts)
          .accounts({ payer: me, taskQueue: otherQueue, task: otherTask })
          .rpc({ skipPreflight: true });

        // tuktuk really does sign the address, because the task is on the queue it was derived
        // from. The cron job names a different queue, and that is what the run is measured on.
        const keys = watched(cronJob);
        const before = await snapshot(keys);
        const ixs = await runTask({
          program: tuktukProgram,
          task: otherTask,
          crankTurner: me,
          nextAvailableTaskIds: [10, 11],
        });
        await expectRefusal(
          [ComputeBudgetProgram.setComputeUnitLimit({ units: 1000000 }), ...ixs],
          {
            program: cronProgram.programId,
            error: "RequireEqViolated",
            code: 2501,
          }
        );
        expectUnchanged(before, await snapshot(keys), keys);
      });

      it("refuses a schedule run not given the free task accounts it needs", async () => {
        const ixs = await runTask({
          program: tuktukProgram,
          task: taskKey(taskQueue, initialTaskId)[0],
          crankTurner: me,
          nextAvailableTaskIds: [],
        });
        await expectRefusal(
          [ComputeBudgetProgram.setComputeUnitLimit({ units: 1000000 }), ...ixs],
          {
            program: cronProgram.programId,
            error: "NotEnoughAccounts",
            code: 6009,
          }
        );
      });

      it("refuses a second schedule chain minted while the recorded one is live", async () => {
        // A cron job carries one schedule chain at a time: while the job records a live
        // schedule task, that task is the only one it answers to.
        const recorded = await recordedTask(cronJob);
        expect(
          await tuktukProgram.account.taskV0.fetchNullable(recorded),
          "precondition: the recorded chain is live"
        ).to.not.be.null;

        const minted = compileTransaction([legacyScheduleIx(cronJob)], []);
        const mintTask = taskKey(taskQueue, 5)[0];
        await tuktukProgram.methods
          .queueTaskV0({
            id: 5,
            trigger: { now: {} },
            transaction: { compiledV0: [minted.transaction] },
            crankReward: null,
            freeTasks: numTasksPerQueueCall + 1,
            description: "queue minted",
          })
          .remainingAccounts(minted.remainingAccounts)
          .accounts({ payer: me, taskQueue, task: mintTask })
          .rpc({ skipPreflight: true });

        // The handover hands it a schedule task, exactly as it would the real chain.
        await run(mintTask, [80, 81]);
        const second = taskKey(taskQueue, 80)[0];
        isV1Schedule(await tuktukProgram.account.taskV0.fetch(second));

        // Running it is what fails, and the cron job is left alone.
        const keys = watched(cronJob);
        const before = await snapshot(keys);
        const ixs = await runTask({
          program: tuktukProgram,
          task: second,
          crankTurner: me,
          nextAvailableTaskIds: [82, 83],
        });
        await expectRefusal(
          [ComputeBudgetProgram.setComputeUnitLimit({ units: 1000000 }), ...ixs],
          {
            program: cronProgram.programId,
            error: "WrongScheduleTask",
            code: 6012,
          }
        );
        expectUnchanged(before, await snapshot(keys), keys);
      });

      it("refuses a schedule run that queues another cron job's transactions", async () => {
        // A record says which job it belongs to in its contents, and a schedule run queues
        // only the records belonging to the job whose funds pay for them.
        const { job: other } = await createCronJobFor(makeid(10), 10000000000);
        await addCronTransaction(other, 0);

        // Reach the record check by adopting a vacancy: retire this job's schedule task first.
        const recorded = await recordedTask(cronJob);
        await dequeue(recorded);

        const [cronSigner, bump] = customSignerKey(taskQueue, [
          Buffer.from("cron"),
          cronJob.toBuffer(),
        ]);
        const ix = await cronProgram.methods
          .queueCronTasksV1()
          .accountsPartial({
            cronJob,
            taskQueue,
            cronSigner,
            recordedScheduleTask: recorded,
          })
          .remainingAccounts([
            {
              pubkey: cronJobTransactionKey(other, 0)[0],
              isSigner: false,
              isWritable: false,
            },
          ])
          .instruction();
        const bumpBuffer = Buffer.alloc(1);
        bumpBuffer.writeUint8(bump);
        const compiled = compileTransaction(
          [ix],
          [[Buffer.from("cron"), cronJob.toBuffer(), bumpBuffer]]
        );
        const mintTask = taskKey(taskQueue, 7)[0];
        await tuktukProgram.methods
          .queueTaskV0({
            id: 7,
            trigger: { now: {} },
            transaction: { compiledV0: [compiled.transaction] },
            crankReward: null,
            freeTasks: numTasksPerQueueCall + 1,
            description: "queue foreign",
          })
          .remainingAccounts(compiled.remainingAccounts)
          .accounts({ payer: me, taskQueue, task: mintTask })
          .rpc({ skipPreflight: true });

        const keys = watched(cronJob);
        const before = await snapshot(keys);
        const ixs = await runTask({
          program: tuktukProgram,
          task: mintTask,
          crankTurner: me,
          nextAvailableTaskIds: [86, 87],
        });
        await expectRefusal(
          [ComputeBudgetProgram.setComputeUnitLimit({ units: 1000000 }), ...ixs],
          {
            program: cronProgram.programId,
            error: "WrongCronTransaction",
            code: 6013,
          }
        );
        expectUnchanged(before, await snapshot(keys), keys);
      });

      it("checks a record's index on a run past the start of the cycle", async () => {
        // Two records per call against three records, so the second run starts at index 2. Every
        // other test here runs one record per call, where the cycle resets to 0 every time and the
        // index check compares 0 against 0 whatever it is written to compare.
        const { job, taskId } = await createCronJobFor(makeid(10), 10000000000, 2);
        await addCronTransaction(job, 0);
        await addCronTransaction(job, 1);
        await addCronTransaction(job, 2);

        await run(taskKey(taskQueue, taskId)[0], [30, 31, 32]);
        const afterFirst = await cronProgram.account.cronJobV0.fetch(job);
        expect(
          afterFirst.currentTransactionId,
          "precondition: the first run queued records 0 and 1"
        ).to.eq(2);

        // The successor names record 2, and the run reads the same index off the job, so this is
        // the first run whose check is not 0 against 0.
        await run(afterFirst.nextScheduleTask, [40, 41, 42]);
        const afterSecond = await cronProgram.account.cronJobV0.fetch(job);
        expect(
          afterSecond.currentTransactionId,
          "the last record was queued, completing the cycle"
        ).to.eq(0);
      });

      // The largest initialize_cron_job_v0 accepts. The account list is assembled by tuktuk's
      // client and the message dedupes keys, so the size is read off the serialized transaction
      // rather than counted.
      for (const perQueueCall of [5]) {
        it(`keeps a schedule run at num_tasks_per_queue_call=${perQueueCall} inside one transaction`, async () => {
          const { taskId } = await createCronJobFor(
            makeid(10),
            10000000000,
            perQueueCall
          );
          const tx = toVersionedTx(
            await populateMissingDraftInfo(provider.connection, {
              feePayer: me,
              instructions: [
                ComputeBudgetProgram.setComputeUnitLimit({ units: 1000000 }),
                ...(await runTask({
                  program: tuktukProgram,
                  task: taskKey(taskQueue, taskId)[0],
                  crankTurner: me,
                  // Ids stay under the queue's capacity of 100 at both sizes, so the
                  // account list is one a run could really name.
                  nextAvailableTaskIds: Array.from(
                    { length: perQueueCall + 1 },
                    (_, i) => 84 + i
                  ),
                })),
              ],
            })
          );
          const signed = await provider.wallet.signTransaction(tx);
          const size = signed.serialize().length;
          console.log(
            `      schedule run at num_tasks_per_queue_call=${perQueueCall}: ${size} bytes of 1232`
          );
          expect(size, "schedule run fits in one transaction").to.be.at.most(
            1232
          );
        });
      }

      it("gates requeue on the recorded schedule task", async () => {
        // A second record so the chain can be part way through an execution, which is the state
        // the reset is for.
        await addCronTransaction(cronJob, 1);
        await run(taskKey(taskQueue, initialTaskId)[0], [20, 21]);
        const midway = await cronProgram.account.cronJobV0.fetch(cronJob);
        expect(midway.currentTransactionId, "precondition: mid-execution").to.eq(1);
        const live = midway.nextScheduleTask;
        expect(live.toBase58()).to.eq(taskKey(taskQueue, 20)[0].toBase58());

        // Refused while the recorded task is live.
        await expectRefusal([await requeueIx(cronJob, live, 50).instruction()], {
          program: cronProgram.programId,
          error: "TaskAlreadyQueued",
          code: 6008,
          account: "next_schedule_task",
        });

        // And a different empty account cannot stand in for the recorded one.
        await expectRefusal(
          [await requeueIx(cronJob, taskKey(taskQueue, 60)[0], 50).instruction()],
          {
            program: cronProgram.programId,
            error: "ConstraintAddress",
            code: 2012,
            account: "next_schedule_task",
          }
        );

        await dequeue(live);
        await requeueIx(cronJob, live, 50).rpc();

        const requeued = await cronProgram.account.cronJobV0.fetch(cronJob);
        expect(requeued.nextScheduleTask.toBase58()).to.eq(
          taskKey(taskQueue, 50)[0].toBase58()
        );
        expect(requeued.currentTransactionId, "execution restarts").to.eq(0);
        expect(requeued.currentExecTs.toNumber()).to.be.greaterThan(
          midway.currentExecTs.toNumber() - 1
        );
        expect(requeued.removedFromQueue).to.be.false;
        isV1Schedule(
          await tuktukProgram.account.taskV0.fetch(taskKey(taskQueue, 50)[0])
        );
      });

      it("stands a cron job down when it cannot fund its tasks, and lets it back in", async () => {
        // Funded only by `init`, so it cannot cover min_crank_reward for the tasks it owes.
        const poorName = makeid(10);
        const { job: poor, taskId } = await createCronJobFor(poorName, 0);
        await addCronTransaction(poor, 0);

        await run(taskKey(taskQueue, taskId)[0], [70, 71]);

        const stood_down = await cronProgram.account.cronJobV0.fetch(poor);
        expect(stood_down.removedFromQueue, "stood down").to.be.true;
        expect(stood_down.nextScheduleTask.toBase58()).to.eq(
          PublicKey.default.toBase58()
        );
        expect(
          await tuktukProgram.account.taskV0.fetchNullable(
            taskKey(taskQueue, 70)[0]
          ),
          "no successor while stood down"
        ).to.be.null;

        // `Pubkey::default()` is the system program, which has data, so the requeue gate has to
        // name it rather than ask whether the account is empty.
        await sendInstructions(provider, [
          SystemProgram.transfer({
            fromPubkey: me,
            toPubkey: poor,
            lamports: 10000000000,
          }),
        ]);
        await requeueIx(poor, PublicKey.default, 72).rpc();
        const back = await cronProgram.account.cronJobV0.fetch(poor);
        expect(back.removedFromQueue).to.be.false;
        expect(back.nextScheduleTask.toBase58()).to.eq(
          taskKey(taskQueue, 72)[0].toBase58()
        );
        isV1Schedule(
          await tuktukProgram.account.taskV0.fetch(taskKey(taskQueue, 72)[0])
        );
      });
    });
  });
});

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
