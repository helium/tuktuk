import { AnchorProvider, Wallet } from "@coral-xyz/anchor";
import {
  compileTransaction,
  init,
  queueTask
} from "@helium/tuktuk-sdk";
import { Connection, Keypair, PublicKey, TransactionInstruction } from "@solana/web3.js";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";
import { initializeTaskQueue, loadKeypair, monitorTask } from "./helpers";

// Solana Memo Program ID
const MEMO_PROGRAM_ID = new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

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
      message: {
        type: "string",
        description: "Message to write in the memo",
        default: "Hello World!",
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
  console.log("Message:", argv.message);

  // Initialize TukTuk program
  const program = await init(provider);
  const queueName = argv.queueName;
  
  const taskQueue = await initializeTaskQueue(program, queueName);

  // Create a simple memo instruction
  const memoInstruction = new TransactionInstruction({
    keys: [],
    data: Buffer.from(argv.message, "utf-8"),
    programId: MEMO_PROGRAM_ID,
  });

  console.log("Compiling instructions...");
  const { transaction, remainingAccounts } = compileTransaction(
    [memoInstruction],
    []
  );

  // Queue the task
  console.log("Queueing task...");
  const { pubkeys: { task }, signature } = await (await queueTask(program, {
    taskQueue,
    args: {
      trigger: { now: {} },
      crankReward: null,
      freeTasks: 0,
      transaction: {
        compiledV0: [transaction],
      },
      description: `memo: ${argv.message}`,
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