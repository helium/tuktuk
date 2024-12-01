import {
  CustomAccountResolver,
  Idl,
  IdlTypes,
  Program,
} from "@coral-xyz/anchor";
import { Tuktuk } from "@helium/tuktuk-idls/lib/types/tuktuk";
import {
  AccountMeta,
  PublicKey,
  TransactionInstruction,
} from "@solana/web3.js";
import { customSignerKey, taskKey, tuktukConfigKey } from "./pdas";
import { getAssociatedTokenAddressSync } from "@solana/spl-token";

export type CompiledTransactionArgV0 =
  IdlTypes<Tuktuk>["compiledTransactionArgV0"];

export type CustomAccountResolverFactory<T extends Idl> = (
  programId: PublicKey
) => CustomAccountResolver<T>;

export function compileTransaction(
  instructions: TransactionInstruction[],
  signerSeeds: Buffer[][],
): { transaction: CompiledTransactionArgV0; remainingAccounts: AccountMeta[] } {
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
        isWritable: false,
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
    },
  };
}

function nextAvailableTaskIds(taskBitmap: Buffer, n: number): number[] {
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

export async function runTask({
  program,
  task,
  rewardsDestinationWallet,
}: {
  program: Program<Tuktuk>;
  task: PublicKey;
  rewardsDestinationWallet: PublicKey;
}) {
  const {
    taskQueue,
    freeTasks,
    transaction: { numRwSigners, numRoSigners, numRw, accounts, signerSeeds },
  } = await program.account.taskV0.fetch(task);
  const taskQueueAcc = await program.account.taskQueueV0.fetch(taskQueue);

  const configAcc = await program.account.tuktukConfigV0.fetch(
    tuktukConfigKey()[0]
  );

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
  const nextAvailable = nextAvailableTaskIds(
    taskQueueAcc.taskBitmap,
    freeTasks
  );
  const freeTasksAccounts = nextAvailable.map((id) => ({
    pubkey: taskKey(taskQueue, id)[0],
    isWritable: true,
    isSigner: false,
  }));

  return program.methods
    .runTaskV0()
    .accounts({
      task,
      rewardsDestination: getAssociatedTokenAddressSync(
        configAcc.networkMint,
        rewardsDestinationWallet
      ),
    })
    .remainingAccounts([...remainingAccounts, ...freeTasksAccounts]);
}
