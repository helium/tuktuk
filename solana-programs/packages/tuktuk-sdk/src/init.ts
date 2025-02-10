import { Tuktuk } from "@helium/tuktuk-idls/lib/types/tuktuk";
import { AnchorProvider, Idl, Program } from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { PROGRAM_ID } from "./constants";

export const init = async (
  provider: AnchorProvider,
  programId: PublicKey = PROGRAM_ID,
  idl?: Idl | null
): Promise<Program<Tuktuk>> => {
  if (!idl) {
    idl = await Program.fetchIdl(programId, provider);
  }

  const tuktuk = new Program<Tuktuk>(
    idl as Tuktuk,
    provider,
  ) as Program<Tuktuk>;

  return tuktuk;
};
