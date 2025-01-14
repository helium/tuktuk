import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import fastify from "fastify";
import * as anchor from "@coral-xyz/anchor";
import {
  compileTransaction,
  customSignerKey,
  init,
  RemoteTaskTransactionV0,
} from "@helium/tuktuk-sdk";
import { Tuktuk } from "@helium/tuktuk-idls/lib/types/tuktuk";
import { sign } from "tweetnacl";
import * as fs from "fs";

const server = fastify({ logger: true });
anchor.setProvider(anchor.AnchorProvider.local(process.env.SOLANA_URL));
const provider = anchor.getProvider() as anchor.AnchorProvider;
const serverWallet = anchor.web3.Keypair.fromSecretKey(
  Buffer.from(JSON.parse(fs.readFileSync(process.env.ANCHOR_WALLET!, "utf-8")))
);

server.get("/health", async () => {
  return { ok: true };
});

let program: anchor.Program<Tuktuk>;
server.post<{
  Body: { task: string; task_queued_at: string; task_queue: string };
}>("/remote", {
  handler: async (request, reply) => {
    const taskQueue = new PublicKey(request.body.task_queue);
    const task = new PublicKey(request.body.task);
    const taskQueuedAt = new anchor.BN(request.body.task_queued_at);
    try {
      const [wallet, bump] = customSignerKey(taskQueue, [Buffer.from("test")]);
      const bumpBuffer = Buffer.alloc(1);
      bumpBuffer.writeUint8(bump);
      // Transfer some tokens from lazy signer to me
      const instructions: TransactionInstruction[] = [
        new TransactionInstruction({
          keys: [{ pubkey: wallet, isSigner: true, isWritable: true }],
          data: Buffer.from("I'm a remote transaction!", "utf-8"),
          programId: new PublicKey(
            "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"
          ),
        }),
      ];
      const { transaction, remainingAccounts } = await compileTransaction(
        instructions,
        [[Buffer.from("test"), bumpBuffer]]
      );
      const remoteTx = new RemoteTaskTransactionV0({
        task,
        taskQueuedAt,
        transaction: {
          ...transaction,
          accounts: remainingAccounts.map((acc) => acc.pubkey),
        },
      });
      const resp = {
        transaction: RemoteTaskTransactionV0.serialize(
          program.coder.types,
          remoteTx
        ).toString("base64"),
        signature: Buffer.from(
          sign.detached(
            Uint8Array.from(
              RemoteTaskTransactionV0.serialize(program.coder.types, remoteTx)
            ),
            serverWallet.secretKey
          )
        ).toString("base64"),
        remaining_accounts: remainingAccounts.map((acc) => ({
          pubkey: acc.pubkey.toBase58(),
          is_signer: acc.isSigner,
          is_writable: acc.isWritable,
        })),
      };
      console.log(resp);
      reply.status(200).send(resp);
    } catch (err) {
      console.error(err);
      reply.status(500).send({
        message: "Request failed",
      });
    }
  },
});

const start = async () => {
  try {
    program = await init(provider);
    await server.listen({
      port: process.env.PORT ? parseInt(process.env.PORT) : 3000,
      host: "0.0.0.0",
    });
  } catch (err) {
    server.log.error(err);
    process.exit(1);
  }
};

start();
