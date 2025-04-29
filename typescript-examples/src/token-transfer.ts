import { AnchorProvider, BN, Wallet } from "@coral-xyz/anchor";
import {
  compileTransaction,
  createTaskQueue,
  customSignerKey,
  customSignerSeedsWithBumps,
  getTaskQueueForName,
  init,
  queueTask
} from "@helium/tuktuk-sdk";
import {
  createAssociatedTokenAccountInstruction,
  createMint,
  createMintToInstruction,
  createTransferInstruction,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import { Connection, Keypair, LAMPORTS_PER_SOL, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";
import { initializeTaskQueue, loadKeypair, monitorTask } from "./helpers";
import { sendInstructions } from "@helium/spl-utils";


async function main() {
  const argv = await yargs(hideBin(process.argv))
    .options({
      queueName: {
        type: "string",
        description: "Name of the task queue",
        demandOption: true,
      },
      walletPath: {
        type: "string",
        description: "Path to the wallet keypair file",
        demandOption: true,
      },
      rpcUrl: {
        type: "string",
        description: "Solana RPC URL",
        demandOption: true,
      },
    })
    .help()
    .alias("help", "h").argv;

  // Load wallet from file
  const keypair: Keypair = loadKeypair(argv.walletPath);

  // Setup connection and provider
  const connection = new Connection(argv.rpcUrl, "confirmed");
  const wallet = new Wallet(keypair);
  const provider = new AnchorProvider(connection, wallet, {
    commitment: "confirmed",
  });

  console.log("Using wallet:", wallet.publicKey.toBase58());
  console.log("RPC URL:", argv.rpcUrl);

  // Initialize TukTuk program
  const program = await init(provider);
  const queueName = argv.queueName;
  const taskQueue = await initializeTaskQueue(program, queueName);


  // Create a PDA wallet associated with the task queue
  const pdaSeeds = [Buffer.from("test")];
  const [pdaWallet] = customSignerKey(taskQueue, pdaSeeds);
  console.log("PDA Wallet:", pdaWallet.toBase58());

  // Create a test mint
  console.log("Creating test mint...");
  const mint = await createMint(
    provider.connection,
    keypair,
    keypair.publicKey,
    keypair.publicKey,
    0
  );
  console.log("Test mint created:", mint.toBase58());

  console.log("Funding PDA wallet...");
  // Setup token accounts
  const pdaTokenAccount = getAssociatedTokenAddressSync(mint, pdaWallet, true);
  const myTokenAccount = getAssociatedTokenAddressSync(
    mint,
    wallet.publicKey,
    true
  );

  await sendInstructions(provider, [
    SystemProgram.transfer({
      fromPubkey: wallet.publicKey,
      toPubkey: pdaWallet,
      lamports: 0.004 * LAMPORTS_PER_SOL,
    }),
    createAssociatedTokenAccountInstruction(wallet.publicKey, pdaTokenAccount, pdaWallet, mint),
    createMintToInstruction(mint, pdaTokenAccount, wallet.publicKey, 10)
  ]);


  // Create instructions for the task
  const instructions: TransactionInstruction[] = [
    createAssociatedTokenAccountInstruction(
      pdaWallet,
      myTokenAccount,
      wallet.publicKey,
      mint
    ),
    createTransferInstruction(pdaTokenAccount, myTokenAccount, pdaWallet, 10),
  ];

  console.log("Compiling instructions...");
  const { transaction, remainingAccounts } = compileTransaction(
    instructions,
    customSignerSeedsWithBumps([pdaSeeds], taskQueue)
  );

  // Compile the instructions and PDA into the args expected by the tuktuk program
  console.log("Queueing task...");
  const { pubkeys: { task }, signature } = await (await queueTask(program, {
    taskQueue,
    args: {
      trigger: { now: {} },
      crankReward: null,
      // 0 tasks will run as a result of this task, ie this task does not return any follow on tasks.
      freeTasks: 0,
      transaction: {
        compiledV0: [transaction],
      },
      description: "test token transfer",
    },
  }))
    .remainingAccounts(remainingAccounts)
    .rpcAndKeys();
  console.log("Task queued! Transaction signature:", signature);
  console.log("Task address:", task.toBase58());

  // Monitor task status
  console.log("\nMonitoring task status...");
  await monitorTask(connection, task);
}

main()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  }); 