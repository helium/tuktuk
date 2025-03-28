import { AnchorProvider, Wallet } from "@coral-xyz/anchor";
import { createCronJob, cronJobTransactionKey, getCronJobForName, init as initCron } from "@helium/cron-sdk";
import {
  compileTransaction,
  init
} from "@helium/tuktuk-sdk";
import { Connection, Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";
import { initializeTaskQueue, loadKeypair } from "./helpers";
import { sendInstructions } from "@helium/spl-utils";

// Solana Memo Program ID
const MEMO_PROGRAM_ID = new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

async function main() {
  const argv = await yargs(hideBin(process.argv))
    .options({
      cronName: {
        type: "string",
        description: "Name of the cron job",
        demandOption: true,
      },
      queueName: {
        type: "string",
        description: "Name of the task queue to use",
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
      fundingAmount: {
        type: "number",
        description: "Amount of SOL to fund the cron job with (in lamports)",
        default: 0.01 * LAMPORTS_PER_SOL,
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
  const cronProgram = await initCron(provider);
  const taskQueue = await initializeTaskQueue(program, argv.queueName);

  // Check if cron job already exists
  let cronJob = await getCronJobForName(cronProgram, argv.cronName);
  if (!cronJob) {
    console.log("Creating new cron job...");
    const { pubkeys: { cronJob: cronJobPubkey } } = await (await createCronJob(cronProgram, {
      tuktukProgram: program,
      taskQueue,
      args: {
        name: argv.cronName,
        schedule: "0 * * * * *", // Run every minute
        // The memo transaction doesn't need to schedule more transactions, so we set this to 0
        freeTasksPerTransaction: 0,
        // We just have one transaction to queue for each cron job, so we set this to 1
        numTasksPerQueueCall: 1,
      }
    }))
      .rpcAndKeys({ skipPreflight: true });
    cronJob = cronJobPubkey;
    console.log("Funding cron job with", argv.fundingAmount / LAMPORTS_PER_SOL, "SOL");
    await sendInstructions(provider, [
      SystemProgram.transfer({
        fromPubkey: keypair.publicKey,
        toPubkey: cronJob,
        lamports: argv.fundingAmount,
      }),
    ]);
    // Create a simple memo instruction
    const memoInstruction = new TransactionInstruction({
      keys: [],
      data: Buffer.from(argv.message, "utf-8"),
      programId: MEMO_PROGRAM_ID,
    });

    // Compile the instruction
    console.log("Compiling instructions...");
    const { transaction, remainingAccounts } = compileTransaction(
      [memoInstruction],
      []
    );

    // Adding memo to the cron job
    await cronProgram.methods
      .addCronTransactionV0({
        index: 0,
        transactionSource: {
          compiledV0: [transaction],
        },
      })
      .accounts({
        payer: keypair.publicKey,
        cronJob,
        cronJobTransaction: cronJobTransactionKey(cronJob, 0)[0],
      })
      .remainingAccounts(remainingAccounts)
      .rpc({ skipPreflight: true });
    console.log(`Cron job created!`);
  } else {
    console.log("Cron job already exists");
  }

  console.log("Cron job address:", cronJob.toBase58());
  console.log(`\nYour memo will be posted every minute. Watch for transactions on task queue ${taskQueue.toBase58()}. To stop the cron job, use the tuktuk-cli:`);
  console.log(`tuktuk -u ${argv.rpcUrl} -w ${argv.walletPath} cron-transaction close --cron-name ${argv.cronName} --id 0`);
  console.log(`tuktuk -u ${argv.rpcUrl} -w ${argv.walletPath} cron close --cron-name ${argv.cronName}`);
}

main()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  }); 