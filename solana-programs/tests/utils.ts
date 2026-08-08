import { Program } from "@coral-xyz/anchor";
import { taskKey } from "@helium/tuktuk-sdk";
import { PublicKey } from "@solana/web3.js";
import { execSync } from "child_process";
import { Tuktuk } from "../target/types/tuktuk";

export const ANCHOR_PATH = "anchor";

export async function ensureIdls() {
  let programs = [
    {
      name: "tuktuk",
      pid: "tuktukUrfhXT6ZT77QTU8RQtvgL967uRuVagWF57zVA",
    },
    {
      name: "cpi_example",
      pid: "cpic9j9sjqvhn2ZX3mqcCgzHKCwiiBTyEszyCwN7MBC",
    },
    {
      name: "cron",
      pid: "cronAjRZnJn3MTP3B9kE62NWDrjSuAPVXf9c4hu4grM",
    },
  ];
  await Promise.all(
    programs.map(async (program) => {
      try {
        execSync(
          `${ANCHOR_PATH} idl init --filepath ${__dirname}/../target/idl/${program.name}.json ${program.pid}`,
          { stdio: "inherit", shell: "/bin/bash" }
        );
      } catch {
        execSync(
          `${ANCHOR_PATH} idl upgrade --filepath ${__dirname}/../target/idl/${program.name}.json ${program.pid}`,
          { stdio: "inherit", shell: "/bin/bash" }
        );
      }
    })
  );
}

export function makeid(length: number) {
  let result = "";
  const characters =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  const charactersLength = characters.length;
  let counter = 0;
  while (counter < length) {
    result += characters.charAt(Math.floor(Math.random() * charactersLength));
    counter += 1;
  }
  return result;
}

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Ids the queue's bitmap marks as holding a task, ascending. */
function usedTaskIds(taskBitmap: Buffer, capacity: number): number[] {
  const ids: number[] = [];
  for (let byteIdx = 0; byteIdx < taskBitmap.length; byteIdx++) {
    for (let bitIdx = 0; bitIdx < 8; bitIdx++) {
      const id = byteIdx * 8 + bitIdx;
      if (id < capacity && (taskBitmap[byteIdx] & (1 << bitIdx)) !== 0) {
        ids.push(id);
      }
    }
  }
  return ids;
}

/**
 * Tasks on the queue a crank turner could run, waiting for at least `minimum` of them.
 *
 * Two reasons a test cannot just name the id it expects. Turners take their free task ids from
 * a randomized start so concurrent turners do not collide, so which id a task landed on is not
 * predictable. And a queue holds tasks that are not yet due — running one fails with
 * TaskNotReady — so the trigger has to be checked.
 *
 * Readiness is judged against the cluster's clock, which is the one the program compares
 * against; this machine's can sit either side of it. A task queued a moment ahead becomes due
 * shortly, so this waits rather than reporting a queue that is merely early as empty.
 */
export async function readyTasks(
  program: Program<Tuktuk>,
  taskQueue: PublicKey,
  minimum: number = 1,
  timeoutMs: number = 20000,
): Promise<PublicKey[]> {
  const { connection } = program.provider;
  const deadline = Date.now() + timeoutMs;
  let ready: PublicKey[] = [];

  do {
    const now =
      (await connection.getBlockTime(await connection.getSlot())) ??
      Math.floor(Date.now() / 1000);
    const queue = await program.account.taskQueueV0.fetch(taskQueue);
    ready = [];
    for (const id of usedTaskIds(queue.taskBitmap, queue.capacity)) {
      const key = taskKey(taskQueue, id)[0];
      const task = await program.account.taskV0.fetchNullable(key);
      // A set bit does not guarantee a task: the id is marked before creation finishes.
      if (!task) continue;
      const triggerTs = task.trigger.timestamp?.[0];
      if (!triggerTs || triggerTs.toNumber() <= now) {
        ready.push(key);
      }
    }
    if (ready.length >= minimum) return ready;
    await new Promise((resolve) => setTimeout(resolve, 500));
  } while (Date.now() < deadline);

  throw new Error(
    `only ${ready.length} of ${minimum} tasks on ${taskQueue.toBase58()} came due within ${timeoutMs}ms`,
  );
}
