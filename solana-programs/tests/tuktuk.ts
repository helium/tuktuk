import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import { Tuktuk } from "../target/types/tuktuk";
import { CpiExample } from "../target/types/cpi_example";
import {
  init,
  taskQueueKey,
  taskQueueNameMappingKey,
  tuktukConfigKey,
  compileTransaction,
  CompiledTransactionV0,
  taskKey,
  runTask,
  customSignerKey,
  RemoteTaskTransactionV0,
  taskQueueAuthorityKey,
} from "@helium/tuktuk-sdk";
import {
  AccountMeta,
  Keypair,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
} from "@solana/web3.js";
import chai from "chai";
import {
  createAtaAndMint,
  createMint,
  populateMissingDraftInfo,
  sendAndConfirmWithRetry,
  sendInstructions,
  toVersionedTx,
  withPriorityFees,
} from "@helium/spl-utils";
import { ensureIdls, makeid } from "./utils";
import {
  createAssociatedTokenAccountInstruction,
  createTransferInstruction,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import { sign } from "tweetnacl";
const { expect } = chai;

describe("tuktuk", () => {
  // Configure the client to use the local cluster.
  anchor.setProvider(anchor.AnchorProvider.local("http://127.0.0.1:8899"));

  let program: Program<Tuktuk>;
  const provider = anchor.getProvider() as anchor.AnchorProvider;
  const me = provider.wallet.publicKey;
  const tuktukConfig = tuktukConfigKey()[0];

  before(async () => {
    await ensureIdls();
    program = await init(provider);
  });

  it("initializes a tuktuk config", async () => {
    if (!(await program.account.tuktukConfigV0.fetchNullable(tuktukConfig))) {
      await program.methods
        .initializeTuktukConfigV0({
          minDeposit: new anchor.BN(100000000),
        })
        .accounts({
          authority: me,
        })
        .rpc();
    }

    const tuktukConfigAcc =
      await program.account.tuktukConfigV0.fetch(tuktukConfig);
    expect(tuktukConfigAcc.authority.toBase58()).to.eq(me.toBase58());
  });

  describe("with a task queue", () => {
    let name: string;
    let taskQueue: PublicKey;
    let transaction: CompiledTransactionV0;
    let remainingAccounts: AccountMeta[];
    const crankReward: anchor.BN = new anchor.BN(1000000000);

    beforeEach(async () => {
      name = makeid(10);
      if (!(await program.account.tuktukConfigV0.fetchNullable(tuktukConfig))) {
        await program.methods
          .initializeTuktukConfigV0({
            minDeposit: new anchor.BN(100000000),
          })
          .accounts({
            authority: me,
          })
          .rpc();
      }
      const config = await program.account.tuktukConfigV0.fetch(tuktukConfig);
      const nextTaskQueueId = config.nextTaskQueueId;
      taskQueue = taskQueueKey(tuktukConfig, nextTaskQueueId)[0];
      await program.methods
        .initializeTaskQueueV0({
          name,
          minCrankReward: crankReward,
          capacity: 65528,
          lookupTables: [],
          staleTaskAge: 100,
        })
        .accounts({
          tuktukConfig,
          payer: me,
          updateAuthority: me,
          taskQueue,
          taskQueueNameMapping: taskQueueNameMappingKey(tuktukConfig, name)[0],
        })
        .rpc();

      await program.methods
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
      const lazySignerAta = await createAtaAndMint(provider, mint, 10, wallet);
      const myAta = getAssociatedTokenAddressSync(mint, me);

      // Transfer some tokens from lazy signer to me
      const instructions: TransactionInstruction[] = [
        createAssociatedTokenAccountInstruction(wallet, myAta, me, mint),
        createTransferInstruction(lazySignerAta, myAta, wallet, 10),
      ];

      const bumpBuffer = Buffer.alloc(1);
      bumpBuffer.writeUint8(bump);
      ({ transaction, remainingAccounts } = await compileTransaction(
        instructions,
        [[Buffer.from("test"), bumpBuffer]],
      ));
    });
    it("allows creating tasks", async () => {
      let task = taskKey(taskQueue, 0)[0];
      await program.methods
        .queueTaskV0({
          id: 0,
          trigger: { now: {} },
          transaction: {
            compiledV0: [transaction],
          },
          crankReward: null,
          freeTasks: 0,
          description: "test",
        })
        .remainingAccounts(remainingAccounts)
        .accounts({
          payer: me,
          taskQueue,
          task,
        })
        .rpc();
      const taskAcc = await program.account.taskV0.fetch(task);
      expect(taskAcc.id).to.eq(0);
      expect(taskAcc.trigger.now).to.not.be.undefined;
      expect(taskAcc.crankReward.toString()).to.eq(crankReward.toString());
    });

    // The compiled-transaction account list a crank turner passes to runTaskV0, with the
    // writability the program recomputes from the compiled header.
    function taskAccounts(): AccountMeta[] {
      const { numRwSigners, numRoSigners, numRw } = transaction;
      return remainingAccounts.map((acc, index) => ({
        pubkey: acc.pubkey,
        isWritable:
          index < numRwSigners ||
          (index >= numRwSigners + numRoSigners &&
            index < numRwSigners + numRoSigners + numRw),
        isSigner: false,
      }));
    }

    async function queueTask(id: number, freeTasks: number) {
      const task = taskKey(taskQueue, id)[0];
      await program.methods
        .queueTaskV0({
          id,
          trigger: { now: {} },
          transaction: { compiledV0: [transaction] },
          crankReward: null,
          freeTasks,
          description: "test",
        })
        .remainingAccounts(remainingAccounts)
        .accounts({ payer: me, taskQueue, task })
        .rpc();
      return task;
    }

    function runTaskWithIds(task: PublicKey, freeTaskIds: number[]) {
      return program.methods
        .runTaskV0({ freeTaskIds })
        .accounts({ task, crankTurner: me })
        .remainingAccounts([
          ...taskAccounts(),
          ...freeTaskIds.map((id) => ({
            pubkey: taskKey(taskQueue, id)[0],
            isWritable: true,
            isSigner: false,
          })),
        ])
        .rpc();
    }

    it("rejects more free task ids than the task declared", async () => {
      const task = await queueTask(2, 0);

      let code: string | undefined;
      try {
        await runTaskWithIds(task, [100]);
      } catch (e: any) {
        code = e?.error?.errorCode?.code ?? JSON.stringify(e);
      }
      expect(code).to.eq("TooManyReturnedTasks");
    });

    it("accepts exactly as many free task ids as the task declared", async () => {
      const task = await queueTask(3, 6);

      await runTaskWithIds(task, [100, 101, 102, 103, 104, 105]);

      // The task ran to completion and closed itself out.
      expect(await program.account.taskV0.fetchNullable(task)).to.be.null;
    });

    it("rejects fewer accounts than the task's transaction needs", async () => {
      const task = await queueTask(5, 0);

      let code: string | undefined;
      try {
        // One short of the compiled transaction's own account list, with no free tasks.
        await program.methods
          .runTaskV0({ freeTaskIds: [] })
          .accounts({ task, crankTurner: me })
          .remainingAccounts(taskAccounts().slice(0, -1))
          .rpc();
      } catch (e: any) {
        code = e?.error?.errorCode?.code ?? JSON.stringify(e);
      }
      expect(code).to.eq("MismatchedFreeTaskCounts");
    });

    it("rejects more accounts than the task's transaction and ids need", async () => {
      const task = await queueTask(6, 0);

      let code: string | undefined;
      try {
        // Surplus accounts are forwarded to every inner CPI, so the count is exact in both
        // directions rather than a lower bound.
        await program.methods
          .runTaskV0({ freeTaskIds: [] })
          .accounts({ task, crankTurner: me })
          .remainingAccounts([
            ...taskAccounts(),
            {
              pubkey: taskKey(taskQueue, 200)[0],
              isWritable: true,
              isSigner: false,
            },
          ])
          .rpc();
      } catch (e: any) {
        code = e?.error?.errorCode?.code ?? JSON.stringify(e);
      }
      expect(code).to.eq("MismatchedFreeTaskCounts");
    });

    it("allows a payer funded task above the queue minimum reward", async () => {
      const task = taskKey(taskQueue, 4)[0];
      const aboveMin = crankReward.muln(2);
      await program.methods
        .queueTaskV0({
          id: 4,
          trigger: { now: {} },
          transaction: { compiledV0: [transaction] },
          crankReward: aboveMin,
          freeTasks: 0,
          description: "test",
        })
        .remainingAccounts(remainingAccounts)
        .accounts({ payer: me, taskQueue, task })
        .rpc();

      const taskAcc = await program.account.taskV0.fetch(task);
      expect(taskAcc.crankReward.toString()).to.eq(aboveMin.toString());
    });

    it("does not let a task borrow the crank turner's signature", async () => {
      const crankTurner = Keypair.generate();
      const attacker = Keypair.generate();
      const stolen = 500000000;
      await sendInstructions(provider, [
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: crankTurner.publicKey,
          lamports: 2000000000,
        }),
      ]);

      // A task whose compiled transaction drains whoever cranks it.
      const { transaction: drainTx, remainingAccounts: drainAccounts } =
        await compileTransaction(
          [
            SystemProgram.transfer({
              fromPubkey: crankTurner.publicKey,
              toPubkey: attacker.publicKey,
              lamports: stolen,
            }),
          ],
          [],
        );

      const task = taskKey(taskQueue, 1)[0];
      await program.methods
        .queueTaskV0({
          id: 1,
          trigger: { now: {} },
          transaction: { compiledV0: [drainTx] },
          crankReward: null,
          freeTasks: 0,
          description: "drain",
        })
        .remainingAccounts(drainAccounts)
        .accounts({ payer: me, taskQueue, task })
        .rpc();

      const balanceBefore = await provider.connection.getBalance(
        crankTurner.publicKey,
      );

      const ixs = await runTask({
        program,
        task,
        crankTurner: crankTurner.publicKey,
      });
      const tx = toVersionedTx(
        await populateMissingDraftInfo(provider.connection, {
          feePayer: crankTurner.publicKey,
          instructions: ixs,
        }),
      );
      await tx.sign([crankTurner]);

      let failed = false;
      try {
        await sendAndConfirmWithRetry(
          provider.connection,
          Buffer.from(tx.serialize()),
          { skipPreflight: true, maxRetries: 0 },
          "confirmed",
        );
      } catch (e) {
        failed = true;
      }
      expect(failed).to.be.true;

      const balanceAfter = await provider.connection.getBalance(
        crankTurner.publicKey,
      );
      expect(balanceBefore - balanceAfter).to.be.lessThan(stolen);
      expect(await provider.connection.getBalance(attacker.publicKey)).to.eq(0);
    });

    it("allows closing a task queue", async () => {
      await program.methods
        .removeQueueAuthorityV0()
        .accounts({
          taskQueue,
          rentRefund: me,
          queueAuthority: me,
        })
        .rpc({ skipPreflight: true });
      await program.methods
        .closeTaskQueueV0()
        .accounts({
          taskQueue,
          rentRefund: me,
          taskQueueNameMapping: taskQueueNameMappingKey(tuktukConfig, name)[0],
        })
        .rpc({ skipPreflight: true });
      const taskQueueAcc =
        await program.account.taskQueueV0.fetchNullable(taskQueue);
      expect(taskQueueAcc).to.be.null;
    });

    describe("with a remote transaction", () => {
      let task: PublicKey;
      let signer = Keypair.generate();
      const crankTurner = Keypair.generate();
      beforeEach(async () => {
        task = taskKey(taskQueue, 0)[0];
        await sendInstructions(provider, [
          SystemProgram.transfer({
            fromPubkey: me,
            toPubkey: crankTurner.publicKey,
            lamports: 10000000000,
          }),
        ]);
        await program.methods
          .queueTaskV0({
            id: 0,
            // trigger: { timestamp: [new anchor.BN(Date.now() / 1000 + 30)] },
            trigger: { now: {} },
            transaction: {
              remoteV0: {
                url: "http://localhost:3002/remote",
                signer: signer.publicKey,
              },
            },
            crankReward: null,
            freeTasks: 0,
            description: "test",
          })
          .accounts({
            payer: crankTurner.publicKey,
            taskQueue,
            task,
          })
          .signers([crankTurner])
          .rpc();
      });

      it("allows running a task", async () => {
        const taskAcc = await program.account.taskV0.fetch(task);

        const ixs = await runTask({
          program,
          task,
          crankTurner: crankTurner.publicKey,
          fetcher: async () => {
            let remoteTx = new RemoteTaskTransactionV0({
              task,
              taskQueuedAt: taskAcc.queuedAt,
              transaction: {
                ...transaction,
                accounts: remainingAccounts.map((acc) => acc.pubkey),
              },
            });
            const serialized = await RemoteTaskTransactionV0.serialize(
              program.coder.accounts,
              remoteTx,
            );
            return {
              remoteTaskTransaction: serialized,
              remainingAccounts: remainingAccounts,
              signature: Buffer.from(
                sign.detached(Uint8Array.from(serialized), signer.secretKey),
              ),
            };
          },
        });
        const tx = toVersionedTx(
          await populateMissingDraftInfo(provider.connection, {
            feePayer: crankTurner.publicKey,
            instructions: ixs,
          }),
        );
        await tx.sign([crankTurner]);
        await sendAndConfirmWithRetry(
          provider.connection,
          Buffer.from(tx.serialize()),
          {
            skipPreflight: true,
            maxRetries: 0,
          },
          "confirmed",
        );
      });
    });

    describe("with a task", () => {
      let task: PublicKey;
      beforeEach(async () => {
        task = taskKey(taskQueue, 0)[0];
        await program.methods
          .queueTaskV0({
            id: 0,
            // trigger: { timestamp: [new anchor.BN(Date.now() / 1000 + 30)] },
            trigger: { now: {} },
            transaction: {
              compiledV0: [transaction],
            },
            crankReward: null,
            freeTasks: 0,
            description: "test",
          })
          .remainingAccounts(remainingAccounts)
          .accounts({
            payer: me,
            task,
            taskQueue,
          })
          .rpc();
      });

      it("allows running a task", async () => {
        const crankTurner = Keypair.generate();
        // Send initial balance and track it
        const initialBalance = 1000000000;
        await sendInstructions(provider, [
          SystemProgram.transfer({
            fromPubkey: me,
            toPubkey: crankTurner.publicKey,
            lamports: initialBalance,
          }),
        ]);

        const crankTurnerBalanceBefore = await provider.connection.getBalance(
          crankTurner.publicKey,
        );

        const ixs = await runTask({
          program,
          task,
          crankTurner: crankTurner.publicKey,
        });
        const tx = toVersionedTx(
          await populateMissingDraftInfo(provider.connection, {
            feePayer: crankTurner.publicKey,
            instructions: ixs,
          }),
        );
        await tx.sign([crankTurner]);
        const taskAcc = await program.account.taskV0.fetch(task);
        const { txid } = await sendAndConfirmWithRetry(
          provider.connection,
          Buffer.from(tx.serialize()),
          {
            skipPreflight: true,
            maxRetries: 0,
          },
          "confirmed",
        );

        const crankTurnerBalanceAfter = await provider.connection.getBalance(
          crankTurner.publicKey,
        );

        // Get the transaction fee
        const txDetails = await provider.connection.getTransaction(txid, {
          commitment: "confirmed",
          maxSupportedTransactionVersion: 0,
        });
        const txFee = txDetails?.meta?.fee || 0;

        // Get task account to check reward
        const protocolFee = Math.floor(taskAcc.crankReward.toNumber() * 0); // 0% protocol fee
        const expectedReward = taskAcc.crankReward.toNumber() - protocolFee;

        // Calculate expected balance change
        const expectedBalanceChange = expectedReward - txFee;
        const actualBalanceChange =
          crankTurnerBalanceAfter - crankTurnerBalanceBefore;

        expect(actualBalanceChange).to.equal(
          expectedBalanceChange,
          `Crank turner balance change incorrect. Expected change: ${expectedBalanceChange}, ` +
            `Actual change: ${actualBalanceChange}, ` +
            `Reward: ${expectedReward}, ` +
            `TX fee: ${txFee}`,
        );
      });

      it("allows dequeueing a task", async () => {
        await program.methods
          .dequeueTaskV0()
          .accounts({
            task,
          })
          .rpc();
        const taskAcc = await program.account.taskV0.fetchNullable(task);
        expect(taskAcc).to.be.null;
      });
    });
  });

  describe("CPI example", () => {
    let cpiProgram: Program<CpiExample>;
    let taskQueue: PublicKey;
    const queueAuthority = PublicKey.findProgramAddressSync(
      [Buffer.from("queue_authority")],
      new PublicKey("cpic9j9sjqvhn2ZX3mqcCgzHKCwiiBTyEszyCwN7MBC"),
    )[0];

    beforeEach(async () => {
      const idl = await Program.fetchIdl(
        new PublicKey("cpic9j9sjqvhn2ZX3mqcCgzHKCwiiBTyEszyCwN7MBC"),
        provider,
      );

      cpiProgram = new Program<CpiExample>(
        idl as CpiExample,
        provider,
      ) as Program<CpiExample>;
      if (!(await program.account.tuktukConfigV0.fetchNullable(tuktukConfig))) {
        await program.methods
          .initializeTuktukConfigV0({
            minDeposit: new anchor.BN(100000000),
          })
          .accounts({
            authority: me,
          })
          .rpc();
      }
      const name = makeid(10);
      const config = await program.account.tuktukConfigV0.fetch(tuktukConfig);
      const nextTaskQueueId = config.nextTaskQueueId;
      taskQueue = taskQueueKey(tuktukConfig, nextTaskQueueId)[0];
      await program.methods
        .initializeTaskQueueV0({
          name,
          minCrankReward: new BN(10000),
          capacity: 100,
          lookupTables: [],
          staleTaskAge: 100,
        })
        .accounts({
          tuktukConfig,
          payer: me,
          updateAuthority: me,
          taskQueue,
          taskQueueNameMapping: taskQueueNameMappingKey(tuktukConfig, name)[0],
        })
        .rpc();
      await program.methods
        .addQueueAuthorityV0()
        .accounts({
          payer: me,
          queueAuthority,
          taskQueue,
        })
        .rpc();
    });
    it("allows scheduling a task", async () => {
      const freeTask1 = taskKey(taskQueue, 0)[0];
      const freeTask2 = taskKey(taskQueue, 1)[0];
      const crankTurner = Keypair.generate();
      const method = await cpiProgram.methods.schedule(0).accounts({
        taskQueue,
        task: freeTask1,
        taskQueueAuthority: taskQueueAuthorityKey(taskQueue, queueAuthority)[0],
      });
      await sendInstructions(provider, [
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: taskQueue!,
          lamports: 1000000000,
        }),
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: crankTurner.publicKey,
          lamports: 1000000000,
        }),
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: queueAuthority!,
          lamports: 1000000000,
        }),
      ]);

      await method.rpc({ skipPreflight: true });
      const ixs = await runTask({
        program,
        task: freeTask1,
        crankTurner: crankTurner.publicKey,
      });
      const tx = toVersionedTx(
        await populateMissingDraftInfo(provider.connection, {
          feePayer: crankTurner.publicKey,
          instructions: ixs,
        }),
      );
      await tx.sign([crankTurner]);
      await sendAndConfirmWithRetry(
        provider.connection,
        Buffer.from(tx.serialize()),
        {
          skipPreflight: true,
          maxRetries: 0,
        },
        "confirmed",
      );
      await sleep(1000);
      const ixs2 = await runTask({
        program,
        task: freeTask2,
        crankTurner: crankTurner.publicKey,
      });
      const tx2 = toVersionedTx(
        await populateMissingDraftInfo(provider.connection, {
          feePayer: crankTurner.publicKey,
          instructions: ixs2,
        }),
      );
      await tx2.sign([crankTurner]);
      await sendAndConfirmWithRetry(
        provider.connection,
        Buffer.from(tx2.serialize()),
        {
          skipPreflight: true,
          maxRetries: 0,
        },
        "confirmed",
      );
    });

    it("allows multiple tasks with account return", async () => {
      let freeTasks: PublicKey[] = [];
      for (let i = 0; i < 21; i++) {
        freeTasks.push(taskKey(taskQueue, i)[0]);
      }
      const crankTurner = Keypair.generate();
      const method = await cpiProgram.methods
        .scheduleWithAccountReturn(0)
        .accounts({
          taskQueue,
          task: freeTasks[0],
          taskQueueAuthority: taskQueueAuthorityKey(
            taskQueue,
            queueAuthority,
          )[0],
        });
      await sendInstructions(provider, [
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: taskQueue!,
          lamports: 1000000000,
        }),
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: crankTurner.publicKey,
          lamports: 1000000000,
        }),
        SystemProgram.transfer({
          fromPubkey: me,
          toPubkey: queueAuthority!,
          lamports: 1000000000,
        }),
      ]);

      await method.rpc({ skipPreflight: true });
      const ixs = await runTask({
        program,
        task: freeTasks[0],
        crankTurner: crankTurner.publicKey,
      });
      const tx = toVersionedTx(
        await populateMissingDraftInfo(provider.connection, {
          feePayer: crankTurner.publicKey,
          instructions: await withPriorityFees({
            instructions: ixs,
            connection: provider.connection,
            feePayer: crankTurner.publicKey,
            computeUnits: 1000000000,
          }),
        }),
      );
      await tx.sign([crankTurner]);
      await sendAndConfirmWithRetry(
        provider.connection,
        Buffer.from(tx.serialize()),
        {
          skipPreflight: true,
          maxRetries: 0,
        },
        "confirmed",
      );
      await sleep(1000);
      const ixs2 = await runTask({
        program,
        task: freeTasks[1],
        crankTurner: crankTurner.publicKey,
      });
      const tx2 = toVersionedTx(
        await populateMissingDraftInfo(provider.connection, {
          feePayer: crankTurner.publicKey,
          instructions: await withPriorityFees({
            instructions: ixs2,
            connection: provider.connection,
            feePayer: crankTurner.publicKey,
            computeUnits: 1000000000,
          }),
        }),
      );
      await tx2.sign([crankTurner]);
      await sendAndConfirmWithRetry(
        provider.connection,
        Buffer.from(tx2.serialize()),
        {
          skipPreflight: true,
          maxRetries: 0,
        },
        "confirmed",
      );
      const ixs3 = await runTask({
        program,
        task: freeTasks[2],
        crankTurner: crankTurner.publicKey,
      });
      const tx3 = toVersionedTx(
        await populateMissingDraftInfo(provider.connection, {
          feePayer: crankTurner.publicKey,
          instructions: ixs3,
        }),
      );
      await tx3.sign([crankTurner]);
      await sendAndConfirmWithRetry(
        provider.connection,
        Buffer.from(tx3.serialize()),
        {
          skipPreflight: true,
          maxRetries: 0,
        },
        "confirmed",
      );
    });

    describe("returned task reward bounds", () => {
      // Matches the min_crank_reward the surrounding beforeEach gives this queue.
      const minCrankReward = new BN(10000);
      let crankTurner: Keypair;

      beforeEach(async () => {
        crankTurner = Keypair.generate();
        await sendInstructions(provider, [
          SystemProgram.transfer({
            fromPubkey: me,
            toPubkey: taskQueue!,
            lamports: 1000000000,
          }),
          SystemProgram.transfer({
            fromPubkey: me,
            toPubkey: crankTurner.publicKey,
            lamports: 1000000000,
          }),
          SystemProgram.transfer({
            fromPubkey: me,
            toPubkey: queueAuthority!,
            lamports: 1000000000,
          }),
        ]);
      });

      async function crank(task: PublicKey) {
        const ixs = await runTask({
          program,
          task,
          crankTurner: crankTurner.publicKey,
          // Pin the child's task id so the tests below can assert on taskKey(taskQueue, 1);
          // by default runTask picks a random free id from the bitmap.
          nextAvailableTaskIds: [1],
        });
        const tx = toVersionedTx(
          await populateMissingDraftInfo(provider.connection, {
            feePayer: crankTurner.publicKey,
            instructions: ixs,
          }),
        );
        await tx.sign([crankTurner]);
        await sendAndConfirmWithRetry(
          provider.connection,
          Buffer.from(tx.serialize()),
          { skipPreflight: true, maxRetries: 0 },
          "confirmed",
        );
      }

      // A parent task returning one child with the given reward. `viaAccount` selects the
      // return-account path rather than the return-data path; both create returned tasks.
      async function scheduleReturning(
        returnCrankReward: BN,
        viaAccount: boolean,
        parentFreeTasks = 1,
      ) {
        const parent = taskKey(taskQueue, 0)[0];
        const args = {
          taskId: 0,
          freeTasks: parentFreeTasks,
          returnCrankReward,
          returnFreeTasks: 0,
        };
        const accounts = {
          taskQueue,
          task: parent,
          taskQueueAuthority: taskQueueAuthorityKey(
            taskQueue,
            queueAuthority,
          )[0],
        };
        await (
          viaAccount
            ? cpiProgram.methods.scheduleReturningViaAccount(args)
            : cpiProgram.methods.scheduleReturning(args)
        )
          .accounts(accounts)
          .rpc({ skipPreflight: true });
        return { parent, child: taskKey(taskQueue, 1)[0] };
      }

      it("creates a returned task at exactly the queue minimum reward", async () => {
        const { parent, child } = await scheduleReturning(
          minCrankReward,
          false,
        );

        await crank(parent);

        const childAcc = await program.account.taskV0.fetch(child);
        expect(childAcc.crankReward.toString()).to.eq(
          minCrankReward.toString(),
        );
      });

      it("rejects a returned task with a reward above the queue minimum", async () => {
        const { parent, child } = await scheduleReturning(
          minCrankReward.muln(2),
          false,
        );

        const before = await provider.connection.getBalance(taskQueue);
        await crank(parent);
        const after = await provider.connection.getBalance(taskQueue);

        expect(await program.account.taskV0.fetchNullable(child)).to.be.null;
        // Rent refunds go to the task's payer, so an untouched queue is exactly unchanged.
        expect(after).to.eq(before);
      });

      it("creates a returned task handed back in an account at the queue minimum", async () => {
        const { parent, child } = await scheduleReturning(minCrankReward, true);

        await crank(parent);

        // The return-account path is a separate call site of create_new_task, so pin that it
        // still creates. Without this, the rejection test below is equally satisfied by that
        // path doing nothing at all.
        const childAcc = await program.account.taskV0.fetch(child);
        expect(childAcc.crankReward.toString()).to.eq(
          minCrankReward.toString(),
        );
      });

      it("fails the run when a returned task has no free task id left", async () => {
        // The parent declares no free tasks but its program still returns one, so no id is left
        // to give it. The turner picks how many ids to supply, so this must fail the run rather
        // than drop the child: otherwise a turner could truncate a task's children — including
        // a recurring task's own reschedule — and still be paid.
        const { parent, child } = await scheduleReturning(
          minCrankReward,
          false,
          0,
        );

        let failed = false;
        try {
          await crank(parent);
        } catch (e) {
          failed = true;
        }
        expect(failed).to.be.true;

        // The run reverted, so the parent is still queued for an honest turner to run.
        expect(await program.account.taskV0.fetchNullable(child)).to.be.null;
        expect(await program.account.taskV0.fetchNullable(parent)).to.not.be
          .null;
      });

      it("rejects an above minimum returned task handed back in an account", async () => {
        const { parent, child } = await scheduleReturning(
          minCrankReward.muln(2),
          true,
        );

        const before = await provider.connection.getBalance(taskQueue);
        await crank(parent);
        const after = await provider.connection.getBalance(taskQueue);

        expect(await program.account.taskV0.fetchNullable(child)).to.be.null;
        expect(after).to.eq(before);
      });
    });
  });
});

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
