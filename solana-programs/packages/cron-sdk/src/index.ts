import { Cron } from "@helium/tuktuk-idls/lib/types/cron";
import { cronJobKey, cronJobNameMappingKey, userCronJobsKey } from "./pdas";
import { IdlTypes, Program } from "@coral-xyz/anchor";
import { ComputeBudgetProgram, PublicKey } from "@solana/web3.js";
import { MethodsBuilder } from "@coral-xyz/anchor/dist/cjs/program/namespace/methods";
import { Tuktuk } from "@helium/tuktuk-idls/lib/types/tuktuk";
import { nextAvailableTaskIds, taskKey } from "@helium/tuktuk-sdk";

export { init } from "./init"
export * from "./constants"
export * from "./pdas"

export async function getCronJobForName(program: Program<Cron>, name: string): Promise<PublicKey | null> {
  const nameMapping = cronJobNameMappingKey(program.provider.wallet!.publicKey, name)[0];
  const cronJobNameMapping = await program.account.cronJobNameMappingV0.fetchNullable(nameMapping);
  if (!cronJobNameMapping) {
    return null;
  }
  return cronJobNameMapping.cronJob;
}

export type InitializeCronJobArgsV0 = IdlTypes<Cron>["initializeCronJobArgsV0"];

export async function createCronJob(program: Program<Cron>, {
  tuktukProgram,
  taskQueue,
  args
}: {
  tuktukProgram: Program<Tuktuk>,
  taskQueue: PublicKey,
  args: InitializeCronJobArgsV0
}): Promise<MethodsBuilder<Cron, Cron["instructions"][3]>> {
  const userCronJobsK = userCronJobsKey(program.provider.wallet!.publicKey)[0];
  const userCronJobs = await program.account.userCronJobsV0.fetchNullable(userCronJobsK);
  const nextCronJobId = userCronJobs?.nextCronJobId ?? 0;
  const taskQueueAcc = await tuktukProgram.account.taskQueueV0.fetch(taskQueue);
  const nextTaskId = nextAvailableTaskIds(taskQueueAcc.taskBitmap, 1, false)[0];
  return program.methods
    .initializeCronJobV0(args)
    .preInstructions([
      ComputeBudgetProgram.setComputeUnitLimit({
        units: 1000000,
      }),
    ])
    .accounts({
      payer: program.provider.wallet!.publicKey,
      authority: program.provider.wallet!.publicKey,
      cronJobNameMapping: cronJobNameMappingKey(program.provider.wallet!.publicKey, args.name)[0],
      taskQueue,
      cronJob: cronJobKey(program.provider.wallet!.publicKey, nextCronJobId)[0],
      task: taskKey(taskQueue, nextTaskId)[0],
    })
}