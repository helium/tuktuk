import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
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
  hashRemainingAccounts,
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

  beforeEach(async () => {
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

    const tuktukConfigAcc = await program.account.tuktukConfigV0.fetch(
      tuktukConfig
    );
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
          capacity: 100,
        })
        .accounts({
          tuktukConfig,
          payer: me,
          queueAuthority: me,
          updateAuthority: me,
          taskQueue,
          taskQueueNameMapping: taskQueueNameMappingKey(tuktukConfig, name)[0],
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
        [[Buffer.from("test"), bumpBuffer]]
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

    it("allows closing a task queue", async () => {
      await program.methods
        .closeTaskQueueV0()
        .accounts({
          taskQueue,
          refund: me,
          taskQueueNameMapping: taskQueueNameMappingKey(tuktukConfig, name)[0],
        })
        .rpc({ skipPreflight: true });
      const taskQueueAcc = await program.account.taskQueueV0.fetchNullable(
        taskQueue
      );
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
            return {
              remoteTaskTransaction: RemoteTaskTransactionV0.serialize(
                program.coder.types,
                remoteTx
              ),
              remainingAccounts: remainingAccounts,
              signature: Buffer.from(
                sign.detached(
                  Uint8Array.from(
                    RemoteTaskTransactionV0.serialize(
                      program.coder.types,
                      remoteTx
                    )
                  ),
                  signer.secretKey
                )
              ),
            };
          },
        });
        const tx = toVersionedTx(
          await populateMissingDraftInfo(provider.connection, {
            feePayer: crankTurner.publicKey,
            instructions: ixs,
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
          })
          .remainingAccounts(remainingAccounts)
          .accounts({
            payer: me,
            taskQueue,
            task,
          })
          .rpc();
      });

      it("allows running a task", async () => {
        const crankTurner = Keypair.generate();
        await sendInstructions(provider, [
          SystemProgram.transfer({
            fromPubkey: me,
            toPubkey: crankTurner.publicKey,
            lamports: 1000000000,
          }),
        ]);
        const ixs = await runTask({
          program,
          task,
          crankTurner: crankTurner.publicKey,
        });
        const tx = toVersionedTx(
          await populateMissingDraftInfo(provider.connection, {
            feePayer: crankTurner.publicKey,
            instructions: ixs,
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
      });

      it("allows dequeueing a task", async () => {
        await program.methods
          .dequeueTaskV0()
          .accounts({
            task,
            taskQueue,
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
      new PublicKey("cpic9j9sjqvhn2ZX3mqcCgzHKCwiiBTyEszyCwN7MBC")
    )[0];

    beforeEach(async () => {
      const idl = await Program.fetchIdl(
        new PublicKey("cpic9j9sjqvhn2ZX3mqcCgzHKCwiiBTyEszyCwN7MBC"),
        provider
      );

      cpiProgram = new Program<CpiExample>(
        idl as CpiExample,
        provider
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
          minCrankReward: new anchor.BN(10),
          capacity: 100,
        })
        .accounts({
          tuktukConfig,
          payer: me,
          queueAuthority,
          updateAuthority: me,
          taskQueue,
          taskQueueNameMapping: taskQueueNameMappingKey(tuktukConfig, name)[0],
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
        })
      );
      await tx2.sign([crankTurner]);
      await sendAndConfirmWithRetry(
        provider.connection,
        Buffer.from(tx2.serialize()),
        {
          skipPreflight: true,
          maxRetries: 0,
        },
        "confirmed"
      );
    });
  });
});

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
