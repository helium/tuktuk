import { IdlTypes, Program } from "@coral-xyz/anchor";
import { customSignerKey, taskKey, taskQueueKey, taskQueueNameMappingKey, tuktukConfigKey } from "./pdas";
import { Tuktuk } from "@helium/tuktuk-idls/lib/types/tuktuk";
import { MethodsBuilder } from "@coral-xyz/anchor/dist/cjs/program/namespace/methods";
import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { compileTransaction } from "./transaction";

export { init } from "./init"
export * from "./constants"
export * from "./pdas"
export * from "./transaction"

export function nextAvailableTaskIds(taskBitmap: Buffer, n: number, random: boolean = true): number[] {
  if (n === 0) {
    return [];
  }

  const availableTaskIds: number[] = [];
  const randStart = random ? Math.floor(Math.random() * taskBitmap.length) : 0;
  for (let byteOffset = 0; byteOffset < taskBitmap.length; byteOffset++) {
    const byteIdx = (byteOffset + randStart) % taskBitmap.length;
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

export const TUKTUK_CONFIG = tuktukConfigKey()[0];

export type TaskQueueV0 = IdlTypes<Tuktuk>["taskQueueV0"];
export type TaskV0 = IdlTypes<Tuktuk>["taskV0"];
export type InitializeTaskQueueArgsV0 = IdlTypes<Tuktuk>["initializeTaskQueueArgsV0"];
export type QueueTaskArgsV0 = IdlTypes<Tuktuk>["queueTaskArgsV0"];

export async function getTaskQueueForName(program: Program<Tuktuk>, name: string): Promise<PublicKey | null> {
  const nameMapping = taskQueueNameMappingKey(TUKTUK_CONFIG, name)[0];
  const taskQueueNameMapping = await program.account.taskQueueNameMappingV0.fetchNullable(nameMapping);
  if (!taskQueueNameMapping) {
    return null;
  }
  return taskQueueNameMapping.taskQueue;
}

export async function createTaskQueue(program: Program<Tuktuk>, args: InitializeTaskQueueArgsV0): Promise<MethodsBuilder<Tuktuk, Tuktuk["instructions"][4]>> {
  const tuktukConfig = await program.account.tuktukConfigV0.fetch(TUKTUK_CONFIG);
  const nextTaskQueueId = tuktukConfig.nextTaskQueueId;
  return program.methods
    .initializeTaskQueueV0(args)
    .accounts({
      tuktukConfig: TUKTUK_CONFIG,
      updateAuthority: program.provider.wallet!.publicKey,
      taskQueue: taskQueueKey(TUKTUK_CONFIG, nextTaskQueueId)[0],
      taskQueueNameMapping: taskQueueNameMappingKey(TUKTUK_CONFIG, args.name)[0],
    })
}

export const customSignerSeedsWithBumps = (signerSeeds: Buffer[][], taskQueue: PublicKey) => {
  return signerSeeds.map((seeds) => {
    return [...seeds, Buffer.from([customSignerKey(taskQueue, seeds)[1]])];
  });
}

export async function queueTask(program: Program<Tuktuk>, {
  taskQueue,
  args,
}: {
  taskQueue: PublicKey,
  args: Omit<QueueTaskArgsV0, "id">,
}): Promise<MethodsBuilder<Tuktuk, Tuktuk["instructions"][6]>> {
  const taskQueueAcc = await program.account.taskQueueV0.fetch(taskQueue);
  const taskId = nextAvailableTaskIds(taskQueueAcc.taskBitmap, 1)[0];
  const task = taskKey(taskQueue, taskId)[0];

  // Queue the task
  return program.methods
    .queueTaskV0({
      ...args,
      id: taskId,
    })
    .accounts({
      payer: program.provider.wallet!.publicKey,
      taskQueue,
      task,
    })
}