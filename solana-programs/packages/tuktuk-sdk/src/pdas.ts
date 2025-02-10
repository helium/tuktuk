import { PublicKey } from "@solana/web3.js";
import { PROGRAM_ID } from "./constants";
import { sha256 } from "js-sha256";
import BN from "bn.js";

export const tuktukConfigKey = (
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  return PublicKey.findProgramAddressSync([Buffer.from("tuktuk_config")], programId);
};

export const taskQueueKey = (
  tuktukConfig: PublicKey,
  id: number,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  const buf = Buffer.alloc(4);
  buf.writeUint32LE(id);
  return PublicKey.findProgramAddressSync(
    [Buffer.from("task_queue"), tuktukConfig.toBuffer(), buf],
    programId
  );
};

export const taskQueueAuthorityKey = (
  taskQueue: PublicKey,
  queueAuthority: PublicKey,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("task_queue_authority"), taskQueue.toBuffer(), queueAuthority.toBuffer()],
    programId
  );
};

export const taskQueueNameMappingKey = (
  tuktukConfig: PublicKey,
  name: string,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  const hash = sha256(name);
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from("task_queue_name_mapping"),
      tuktukConfig.toBuffer(),
      Buffer.from(hash, "hex"),
    ],
    programId
  );
};

export const taskKey = (
  taskQueue: PublicKey,
  id: number,
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  const buf = Buffer.alloc(2);
  buf.writeUint16LE(id);
  return PublicKey.findProgramAddressSync(
    [Buffer.from("task"), taskQueue.toBuffer(), buf],
    programId
  );
};


export const customSignerKey = (
  taskQueue: PublicKey,
  signerSeeds: Buffer[],
  programId: PublicKey = PROGRAM_ID
): [PublicKey, number] => {
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from("custom"),
      taskQueue.toBuffer(),
      ...signerSeeds,
    ],
    programId
  );
};
