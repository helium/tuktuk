use solana_sdk::{instruction::Instruction, signature::Keypair};

pub struct InstructionBundle {
    pub instructions: Vec<Instruction>,
    pub signers: Vec<Keypair>,
}
