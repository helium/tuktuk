import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Tuktuk } from "../target/types/tuktuk";
import {
  init,
  taskQueueKey,
  taskQueueNameMappingKey,
  tuktukConfigKey,
  compileTransaction,
  CompiledTransactionArgV0,
  taskKey,
  runTask,
  customSignerKey,
} from "@helium/tuktuk-sdk";
import {
  AccountMeta,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
} from "@solana/web3.js";
import chai from "chai";
import {
  createAtaAndMint,
  createMint,
  sendInstructions,
} from "@helium/spl-utils";
import { ensureIdls, makeid } from "./utils";
import {
  createAssociatedTokenAccountInstruction,
  createTransferInstruction,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
const { expect } = chai;

describe("node-manager", () => {
  // Configure the client to use the local cluster.
  anchor.setProvider(anchor.AnchorProvider.local("http://127.0.0.1:8899"));

  let program: Program<Tuktuk>;
  const provider = anchor.getProvider() as anchor.AnchorProvider;
  const me = provider.wallet.publicKey;
  let hnt: PublicKey;
  const tuktukConfig = tuktukConfigKey()[0];

  beforeEach(async () => {
    await ensureIdls();
    program = await init(provider);
    hnt = await createMint(provider, 8, me);
  });

  it("initializes a tuktuk config", async () => {
    if (!(await program.account.tuktukConfigV0.fetchNullable(tuktukConfig))) {
      await program.methods
        .initializeTuktukConfigV0({
          minDeposit: new anchor.BN(100000000),
        })
        .accounts({
          networkMint: hnt,
          authority: me,
        })
        .rpc();
    }

    const tuktukConfigAcc = await program.account.tuktukConfigV0.fetch(
      tuktukConfig
    );
    expect(tuktukConfigAcc.networkMint.toBase58()).to.eq(hnt.toBase58());
    expect(tuktukConfigAcc.authority.toBase58()).to.eq(me.toBase58());
  });

  describe("with a task queue", () => {
    const name = makeid(10);
    let taskQueue: PublicKey;
    let transaction: CompiledTransactionArgV0;
    let remainingAccounts: AccountMeta[];
    const crankReward: anchor.BN = new anchor.BN(1000000000);

    beforeEach(async () => {
      if (!(await program.account.tuktukConfigV0.fetchNullable(tuktukConfig))) {
        await program.methods
          .initializeTuktukConfigV0({
            minDeposit: new anchor.BN(100000000),
          })
          .accounts({
            networkMint: hnt,
            authority: me,
          })
          .rpc();
      }
      const config = await program.account.tuktukConfigV0.fetch(tuktukConfig);
      const nextTaskQueueId = config.nextTaskQueueId;
      taskQueue = taskQueueKey(tuktukConfig, nextTaskQueueId)[0];
      await createAtaAndMint(
        provider,
        config.networkMint,
        new anchor.BN(1000000000),
        taskQueue
      );
      await createAtaAndMint(provider, config.networkMint, 1, me);
      await program.methods
        .initializeTaskQueueV0({
          name,
          crankReward,
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

    describe("with a task", () => {
      let task: PublicKey;
      beforeEach(async () => {
        task = taskKey(taskQueue, 0)[0];
        await program.methods
          .queueTaskV0({
            id: 0,
            // trigger: { timestamp: [new anchor.BN(Date.now() / 1000 + 30)] },
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
      });

      it("allows running a task", async () => {
        console.log(
          await (
            await runTask({
              program,
              task,
              rewardsDestinationWallet: me,
            })
          ).rpc({ skipPreflight: true })
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
});
