import { PublicKey } from "@solana/web3.js";
import { PROGRAM_ID } from "./constants";
import { sha256 } from "js-sha256";
import BN from "bn.js";

export const userCronJobsKey = (
  authority: PublicKey,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  return PublicKey.findProgramAddressSync([Buffer.from("user_cron_jobs"), authority.toBuffer()], programId);
};

export const cronJobKey = (
  authority: PublicKey,
  id: number,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  const buf = Buffer.alloc(4);
  buf.writeUint32LE(id);
  return PublicKey.findProgramAddressSync(
    [Buffer.from("cron_job"), authority.toBuffer(), buf],
    programId
  );
};

export const cronJobTransactionKey = (
  cronJob: PublicKey,
  id: number,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  const buf = Buffer.alloc(4);
  buf.writeUint32LE(id);
  return PublicKey.findProgramAddressSync(
    [Buffer.from("cron_job_transaction"), cronJob.toBuffer(), buf],
    programId
  );
};

export const cronJobNameMappingKey = (
  authority: PublicKey,
  name: string,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  const hash = sha256(name);
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from("cron_job_name_mapping"),
      authority.toBuffer(),
      Buffer.from(hash, "hex"),
    ],
    programId
  );
};
