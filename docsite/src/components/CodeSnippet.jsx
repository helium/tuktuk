"use client"

import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Copy } from "lucide-react"

export function CodeSnippet() {
    const [copied, setCopied] = useState(false)

    const copyToClipboard = (text) => {
        navigator.clipboard.writeText(text)
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
    }

    const memoCode = `import { AnchorProvider, Wallet } from "@coral-xyz/anchor";
import {
  compileTransaction,
  init,
  queueTask
} from "@helium/tuktuk-sdk";
import { Connection, Keypair, PublicKey, TransactionInstruction } from "@solana/web3.js";

// Solana Memo Program ID
const MEMO_PROGRAM_ID = new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

// Initialize provider with your wallet and connection
const connection = new Connection("YOUR_RPC_URL", "confirmed");
const wallet = new Wallet(YOUR_KEYPAIR);
const provider = new AnchorProvider(connection, wallet, {
  commitment: "confirmed",
});

// Initialize TukTuk program
const program = await init(provider);
const taskQueue = await initializeTaskQueue(program, "YOUR_QUEUE_NAME");

// Create a simple memo instruction
const memoInstruction = new TransactionInstruction({
  keys: [],
  data: Buffer.from("Hello World!", "utf-8"),
  programId: MEMO_PROGRAM_ID,
});

// Compile the instruction for TukTuk
const { transaction, remainingAccounts } = compileTransaction(
  [memoInstruction],
  []
);

// Queue the task for immediate execution
const { pubkeys: { task }, signature } = await (await queueTask(program, {
  taskQueue,
  args: {
    trigger: { now: {} },
    crankReward: null,
    freeTasks: 0,
    transaction: {
      compiledV0: [transaction],
    },
    description: "Send memo message",
  },
}))
  .remainingAccounts(remainingAccounts)
  .rpcAndKeys();

console.log("Task queued! Transaction signature:", signature);
console.log("Task address:", task.toBase58());`

    const cronJobCode = `import { AnchorProvider, Wallet } from "@coral-xyz/anchor";
import { createCronJob, cronJobTransactionKey, init as initCron } from "@helium/cron-sdk";
import {
  compileTransaction,
  init
} from "@helium/tuktuk-sdk";
import { Connection, Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import { sendInstructions } from "@helium/spl-utils";

// Solana Memo Program ID
const MEMO_PROGRAM_ID = new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

// Initialize provider with your wallet and connection
const connection = new Connection("YOUR_RPC_URL", "confirmed");
const wallet = new Wallet(YOUR_KEYPAIR);
const provider = new AnchorProvider(connection, wallet, {
  commitment: "confirmed",
});

// Initialize TukTuk and Cron programs
const program = await init(provider);
const cronProgram = await initCron(provider);
const taskQueue = await initializeTaskQueue(program, "YOUR_QUEUE_NAME");

// Create a new cron job
const { pubkeys: { cronJob } } = await (await createCronJob(cronProgram, {
  tuktukProgram: program,
  taskQueue,
  args: {
    name: "YOUR_CRON_JOB_NAME",
    schedule: "0 * * * * *", // Run every minute
    freeTasksPerTransaction: 0,
    numTasksPerQueueCall: 1,
  }
}))
  .rpcAndKeys({ skipPreflight: true });

// Fund the cron job
await sendInstructions(provider, [
  SystemProgram.transfer({
    fromPubkey: wallet.publicKey,
    toPubkey: cronJob,
    lamports: 0.01 * LAMPORTS_PER_SOL,
  }),
]);

// Create a memo instruction
const memoInstruction = new TransactionInstruction({
  keys: [],
  data: Buffer.from("Scheduled message", "utf-8"),
  programId: MEMO_PROGRAM_ID,
});

// Compile the instruction
const { transaction, remainingAccounts } = compileTransaction(
  [memoInstruction],
  []
);

// Add the transaction to the cron job
await cronProgram.methods
  .addCronTransactionV0({
    index: 0,
    transactionSource: {
      compiledV0: [transaction],
    },
  })
  .accounts({
    payer: wallet.publicKey,
    cronJob,
    cronJobTransaction: cronJobTransactionKey(cronJob, 0)[0],
  })
  .remainingAccounts(remainingAccounts)
  .rpc({ skipPreflight: true });

console.log("Cron job created:", cronJob.toBase58());`

    const tokenTransferCode = `import { AnchorProvider, BN, Wallet } from "@coral-xyz/anchor";
import {
  compileTransaction,
  customSignerKey,
  customSignerSeedsWithBumps,
  init,
  queueTask
} from "@helium/tuktuk-sdk";
import {
  createAssociatedTokenAccountInstruction,
  createTransferInstruction,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";

// Initialize provider with your wallet and connection
const connection = new Connection("YOUR_RPC_URL", "confirmed");
const wallet = new Wallet(YOUR_KEYPAIR);
const provider = new AnchorProvider(connection, wallet, {
  commitment: "confirmed",
});

// Initialize TukTuk program
const program = await init(provider);
const taskQueue = await initializeTaskQueue(program, "YOUR_QUEUE_NAME");

// Create a PDA wallet associated with the task queue
const pdaSeeds = [Buffer.from("wallet-seed")];
const [pdaWallet] = customSignerKey(taskQueue, pdaSeeds);

// Setup token accounts (assuming the PDA wallet already has tokens)
const tokenMint = new PublicKey("YOUR_TOKEN_MINT_ADDRESS");
const pdaTokenAccount = getAssociatedTokenAddressSync(tokenMint, pdaWallet, true);
const recipientTokenAccount = getAssociatedTokenAddressSync(
  tokenMint,
  new PublicKey("RECIPIENT_ADDRESS"),
  true
);

// Create instructions for the task
const instructions = [
  // Create the recipient token account if it doesn't exist yet
  createAssociatedTokenAccountInstruction(
    pdaWallet,
    recipientTokenAccount,
    new PublicKey("RECIPIENT_ADDRESS"),
    tokenMint
  ),
  // Transfer tokens from the PDA to the recipient
  createTransferInstruction(
    pdaTokenAccount, 
    recipientTokenAccount, 
    pdaWallet, 
    1000000 // Amount to transfer (adjust decimal places as needed)
  ),
];

// Compile the instructions with PDA signer information
const { transaction, remainingAccounts } = compileTransaction(
  instructions,
  customSignerSeedsWithBumps([pdaSeeds], taskQueue)
);

// Queue the task for execution
const { pubkeys: { task }, signature } = await (await queueTask(program, {
  taskQueue,
  args: {
    trigger: { now: {} },
    crankReward: null,
    freeTasks: 0,
    transaction: {
      compiledV0: [transaction],
    },
    description: "Scheduled token transfer",
  },
}))
  .remainingAccounts(remainingAccounts)
  .rpcAndKeys();

console.log("Token transfer task queued:", task.toBase58());`

    return (
        <div className="rounded-lg border bg-card text-card-foreground shadow-sm">
            <div className="flex flex-col space-y-1.5 p-6">
                <h3 className="text-2xl font-semibold leading-none tracking-tight">Get Started with Examples</h3>
                <p className="text-sm text-muted-foreground">
                    Explore these practical examples to start building with TukTuk SDK.
                </p>
            </div>
            <Tabs defaultValue="memo" className="w-full">
                <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between px-6">
                    <TabsList className="h-auto flex-wrap gap-2">
                        <TabsTrigger className="whitespace-nowrap" value="memo">Simple Memo</TabsTrigger>
                        <TabsTrigger className="whitespace-nowrap" value="cronJob">Scheduled Task</TabsTrigger>
                        <TabsTrigger className="whitespace-nowrap" value="tokenTransfer">Token Transfer</TabsTrigger>
                    </TabsList>
                    <Button
                        variant="ghost"
                        size="sm"
                        className="mt-2 sm:mt-0"
                        onClick={() => {
                            const codeMap = {
                                memo: memoCode,
                                cronJob: cronJobCode,
                                tokenTransfer: tokenTransferCode,
                            }
                            const activeTab =
                                document.querySelector('[role="tablist"] [data-state="active"]')?.getAttribute("value") || "memo"
                            copyToClipboard(codeMap[activeTab])
                        }}
                    >
                        {copied ? "Copied!" : <Copy className="h-4 w-4" />}
                    </Button>
                </div>
                <TabsContent value="memo" className="p-6 pt-2">
                    <pre className="rounded-md bg-slate-950 p-4 overflow-x-auto text-sm text-slate-50">
                        <code>{memoCode}</code>
                    </pre>
                </TabsContent>
                <TabsContent value="cronJob" className="p-6 pt-2">
                    <pre className="rounded-md bg-slate-950 p-4 overflow-x-auto text-sm text-slate-50">
                        <code>{cronJobCode}</code>
                    </pre>
                </TabsContent>
                <TabsContent value="tokenTransfer" className="p-6 pt-2">
                    <pre className="rounded-md bg-slate-950 p-4 overflow-x-auto text-sm text-slate-50">
                        <code>{tokenTransferCode}</code>
                    </pre>
                </TabsContent>
            </Tabs>
        </div>
    )
}
