import { Cron } from "@helium/tuktuk-idls/lib/types/cron";
import { AnchorProvider, Idl, Program } from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { PROGRAM_ID } from "./constants";

export const init = async (
  provider: AnchorProvider,
  programId: PublicKey = PROGRAM_ID,
  idl?: Idl | null
): Promise<Program<Cron>> => {
  if (!idl) {
    idl = await Program.fetchIdl(programId, provider);
  }

  const cron = new Program<Cron>(idl as Cron, provider) as Program<Cron>;

  return cron;
};
