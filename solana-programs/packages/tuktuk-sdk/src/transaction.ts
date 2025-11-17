import {
  AccountsCoder,
  BN,
  CustomAccountResolver,
  Idl,
  IdlTypes,
  Program,
  TypesCoder,
} from "@coral-xyz/anchor";
import { Tuktuk } from "@helium/tuktuk-idls/lib/types/tuktuk";
import {
  AccountMeta,
  Ed25519Program,
  PublicKey,
  TransactionInstruction,
} from "@solana/web3.js";
import axios from "axios";
import { sha256 } from "js-sha256";
import { taskKey } from "./pdas";

export function verificationHash(
  task: PublicKey,
  taskQueuedAt: BN,
  remainingAccounts: AccountMeta[]
): Buffer {
  let taskQueuedAtBuf = Buffer.alloc(8);
  taskQueuedAtBuf.writeBigInt64LE(BigInt(taskQueuedAt.toString()));
  return Buffer.from(
    sha256(
      Buffer.concat([
        task.toBuffer(),
        taskQueuedAtBuf,
        Buffer.concat(
          remainingAccounts.map((acc) =>
            Buffer.concat([
              acc.pubkey.toBuffer(),
              Buffer.from([acc.isWritable ? 1 : 0, acc.isSigner ? 1 : 0]),
            ])
          )
        ),
      ])
    ),
    "hex"
  );
}

export class RemoteTaskTransactionV0 {
  transaction: CompiledTransactionV0;
  verificationHash: Buffer;

  constructor(fields: {
    task: PublicKey;
    taskQueuedAt: BN;
    transaction: CompiledTransactionV0;
  }) {
    this.verificationHash = verificationHash(
      fields.task,
      fields.taskQueuedAt,
      fields.transaction.accounts.map((acc, index) => {
        const isWritable =
          index < fields.transaction.numRwSigners ||
          (index >=
            fields.transaction.numRwSigners + fields.transaction.numRoSigners &&
            index <
              fields.transaction.numRwSigners +
                fields.transaction.numRoSigners +
                fields.transaction.numRw);
        return {
          pubkey: acc,
          isWritable,
          isSigner: false,
        };
      })
    );
    this.transaction = { ...fields.transaction, accounts: [] };
  }

  static async serialize(
    coder: AccountsCoder,
    value: RemoteTaskTransactionV0
  ): Promise<Buffer> {
    return coder.encode("remoteTaskTransactionV0", value);
  }
}

function sighash(nameSpace: string, name: string): Buffer {
  let preimage = `${nameSpace}:${name}`;
  return Buffer.from(sha256(preimage)).subarray(0, 8);
}

export type CompiledTransactionV0 = IdlTypes<Tuktuk>["compiledTransactionV0"];

export type CustomAccountResolverFactory<T extends Idl> = (
  programId: PublicKey
) => CustomAccountResolver<T>;

export function compileTransaction(
  instructions: TransactionInstruction[],
  signerSeeds: Buffer[][]
): { transaction: CompiledTransactionV0; remainingAccounts: AccountMeta[] } {
  let pubkeysToMetadata: Record<
    string,
    { isSigner: boolean; isWritable: boolean }
  > = {};
  instructions.forEach((ix) => {
    pubkeysToMetadata[ix.programId.toBase58()] ||= {
      isSigner: false,
      isWritable: false,
    };
    ix.keys.forEach((k) => {
      pubkeysToMetadata[k.pubkey.toBase58()] = {
        isWritable:
          pubkeysToMetadata[k.pubkey.toBase58()]?.isWritable || k.isWritable,
        isSigner:
          pubkeysToMetadata[k.pubkey.toBase58()]?.isSigner || k.isSigner,
      };
    });
  });

  // Writable signers first. Then ro signers. Then rw non signers. Then ro
  const sortedAccounts = Object.keys(pubkeysToMetadata).sort((a, b) => {
    const aMeta = pubkeysToMetadata[a];
    const bMeta = pubkeysToMetadata[b];

    if (aMeta.isSigner && bMeta.isSigner) {
      if (aMeta.isWritable) {
        return -1;
      } else if (bMeta.isWritable) {
        return 1;
      } else {
        return 0;
      }
    } else if (bMeta.isSigner) {
      return 1;
    } else if (aMeta.isSigner) {
      return -1;
    } else if (aMeta.isWritable && bMeta.isWritable) {
      return 0;
    } else if (aMeta.isWritable) {
      return -1;
    } else if (bMeta.isWritable) {
      return 1;
    } else {
      return 0;
    }
  });

  let numRwSigners = 0;
  let numRoSigners = 0;
  let numRw = 0;
  sortedAccounts.forEach((k) => {
    const { isWritable, isSigner } = pubkeysToMetadata[k];
    if (isSigner && isWritable) {
      numRwSigners++;
    } else if (isSigner && !isWritable) {
      numRoSigners++;
    } else if (isWritable) {
      numRw++;
    }
  });
  const accountsToIndex = sortedAccounts.reduce((acc, k, i) => {
    acc[k] = i;
    return acc;
  }, {} as Record<string, number>);

  return {
    remainingAccounts: sortedAccounts.map((k) => {
      return {
        pubkey: new PublicKey(k),
        isSigner: false,
        isWritable: pubkeysToMetadata[k].isWritable,
      };
    }),
    transaction: {
      numRoSigners,
      numRwSigners,
      numRw,
      instructions: instructions.map((ix) => {
        return {
          programIdIndex: accountsToIndex[ix.programId.toBase58()],
          accounts: Buffer.from(
            ix.keys.map((k) => accountsToIndex[k.pubkey.toBase58()])
          ),
          data: Buffer.from(ix.data),
        };
      }),
      signerSeeds,
      accounts: [],
    },
  };
}

function nextAvailableTaskIds(taskBitmap: Buffer, n: number): number[] {
  if (n === 0) {
    return [];
  }

  const availableTaskIds: number[] = [];
  for (let byteIdx = 0; byteIdx < taskBitmap.length; byteIdx++) {
    const byte = taskBitmap[byteIdx];
    if (byte !== 0xff) {
      // If byte is not all 1s
      for (let bitIdx = 0; bitIdx < 8; bitIdx++) {
        if ((byte & (1 << bitIdx)) === 0) {
          availableTaskIds.push(byteIdx * 8 + bitIdx);
          if (availableTaskIds.length === n) {
            return availableTaskIds;
          }
        }
      }
    }
  }
  return availableTaskIds;
}

async function defaultFetcher({
  task,
  taskQueuedAt,
  url,
  taskQueue,
}: {
  task: PublicKey;
  taskQueue: PublicKey;
  taskQueuedAt: BN;
  url: string;
}): Promise<{
  remoteTaskTransaction: Buffer;
  remainingAccounts: AccountMeta[];
  signature: Buffer;
}> {
  const resp = await axios.post(url, {
    task: task.toBase58(),
    task_queued_at: taskQueuedAt.toString(),
    task_queue: taskQueue.toBase58(),
  });
  const { transaction: txB64, signature, remaining_accounts } = resp.data;
  const remainingAccounts = remaining_accounts.map((acc) => {
    return {
      pubkey: new PublicKey(acc.pubkey),
      isWritable: acc.is_writable,
      isSigner: acc.is_signer,
    };
  });
  return {
    remoteTaskTransaction: Buffer.from(txB64, "base64"),
    remainingAccounts,
    signature: Buffer.from(signature, "base64"),
  };
}

export async function runTask({
  program,
  task,
  crankTurner,
  fetcher = defaultFetcher,
  nextAvailableTaskIds: argsNextAvailableTaskIds,
}: {
  program: Program<Tuktuk>;
  task: PublicKey;
  crankTurner: PublicKey;
  nextAvailableTaskIds?: number[];
  fetcher?: ({
    task,
    taskQueuedAt,
    url,
  }: {
    task: PublicKey;
    taskQueuedAt: BN;
    taskQueue: PublicKey;
    url: string;
  }) => Promise<{
    remoteTaskTransaction: Buffer;
    remainingAccounts: AccountMeta[];
    signature: Buffer;
  }>;
}): Promise<TransactionInstruction[]> {
  const { taskQueue, freeTasks, transaction, queuedAt } =
    await program.account.taskV0.fetch(task);
  const taskQueueAcc = await program.account.taskQueueV0.fetch(taskQueue);
  if (transaction.compiledV0) {
    const { numRwSigners, numRoSigners, numRw, accounts } =
      transaction.compiledV0[0];
    const remainingAccounts = accounts.map((acc, index) => {
      return {
        pubkey: acc,
        isWritable:
          index < numRwSigners ||
          (index >= numRwSigners + numRoSigners &&
            index < numRwSigners + numRoSigners + numRw),
        isSigner: false,
      };
    });

    const nextAvailable =
      argsNextAvailableTaskIds?.slice(0, freeTasks) ||
      nextAvailableTaskIds(taskQueueAcc.taskBitmap, freeTasks);
    const freeTasksAccounts = nextAvailable.map((id) => ({
      pubkey: taskKey(taskQueue, id)[0],
      isWritable: true,
      isSigner: false,
    }));

    return [
      await program.methods
        .runTaskV0({
          freeTaskIds: nextAvailable,
        })
        .accounts({
          task,
          crankTurner,
        })
        .remainingAccounts([...remainingAccounts, ...freeTasksAccounts])
        .instruction(),
    ];
  } else {
    const nextAvailable =
      argsNextAvailableTaskIds?.slice(0, freeTasks) ||
      nextAvailableTaskIds(taskQueueAcc.taskBitmap, freeTasks);
    const freeTasksAccounts = nextAvailable.map((id) => ({
      pubkey: taskKey(taskQueue, id)[0],
      isWritable: true,
      isSigner: false,
    }));

    const { remoteTaskTransaction, remainingAccounts, signature } =
      await fetcher({
        task,
        taskQueuedAt: queuedAt,
        url: transaction.remoteV0.url,
        taskQueue,
      });

    return [
      Ed25519Program.createInstructionWithPublicKey({
        publicKey: transaction.remoteV0.signer.toBytes(),
        message: remoteTaskTransaction,
        signature,
      }),
      await program.methods
        .runTaskV0({
          freeTaskIds: nextAvailable,
        })
        .accounts({
          task,
          crankTurner,
        })
        .remainingAccounts([...remainingAccounts, ...freeTasksAccounts])
        .instruction(),
    ];
  }
}
