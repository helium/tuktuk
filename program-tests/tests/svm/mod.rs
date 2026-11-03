//! The in-process SVM, on the program crates' types.
//!
//! The first litesvm that loads an SBPF v3 ELF (0.13.1) speaks the agave 4.x type crates, while
//! the programs and these tests are on solana 2.2. This wrapper keeps the tests on the 2.2 types
//! and converts at the boundary, by bytes. Only the calls the tests make are wrapped.

use litesvm::{error::LiteSVMError, types::TransactionResult};
use solana_sdk::{account::Account, hash::Hash, pubkey::Pubkey, transaction::Transaction};

pub use litesvm::types::{FailedTransactionMetadata, TransactionMetadata};
pub use solana_clock::Clock;
pub use solana_instruction_error::InstructionError;
pub use solana_transaction_error::TransactionError;

pub struct LiteSVM(litesvm::LiteSVM);

fn address(key: &Pubkey) -> solana_address::Address {
    solana_address::Address::new_from_array(key.to_bytes())
}

fn pubkey(address: &solana_address::Address) -> Pubkey {
    Pubkey::new_from_array(address.to_bytes())
}

/// The same transaction on litesvm's types. The signatures were made over the message bytes,
/// which are unchanged, so they still verify.
fn transaction(tx: Transaction) -> solana_transaction::Transaction {
    let message = tx.message;
    solana_transaction::Transaction {
        signatures: tx
            .signatures
            .iter()
            .map(|s| {
                solana_signature::Signature::from(
                    <[u8; 64]>::try_from(s.as_ref()).expect("a signature is 64 bytes"),
                )
            })
            .collect(),
        message: solana_message::Message {
            header: solana_message::MessageHeader {
                num_required_signatures: message.header.num_required_signatures,
                num_readonly_signed_accounts: message.header.num_readonly_signed_accounts,
                num_readonly_unsigned_accounts: message.header.num_readonly_unsigned_accounts,
            },
            account_keys: message.account_keys.iter().map(address).collect(),
            recent_blockhash: solana_hash::Hash::new_from_array(
                message.recent_blockhash.to_bytes(),
            ),
            instructions: message
                .instructions
                .into_iter()
                .map(
                    |ix| solana_message::compiled_instruction::CompiledInstruction {
                        program_id_index: ix.program_id_index,
                        accounts: ix.accounts,
                        data: ix.data,
                    },
                )
                .collect(),
        },
    }
}

impl LiteSVM {
    pub fn new() -> Self {
        Self(litesvm::LiteSVM::new())
    }

    pub fn add_program(&mut self, program_id: Pubkey, program: &[u8]) -> Result<(), LiteSVMError> {
        self.0.add_program(address(&program_id), program)
    }

    pub fn airdrop(&mut self, key: &Pubkey, lamports: u64) -> TransactionResult {
        self.0.airdrop(&address(key), lamports)
    }

    pub fn get_account(&self, key: &Pubkey) -> Option<Account> {
        self.0.get_account(&address(key)).map(|a| Account {
            lamports: a.lamports,
            data: a.data,
            owner: pubkey(&a.owner),
            executable: a.executable,
            rent_epoch: a.rent_epoch,
        })
    }

    pub fn set_account(&mut self, key: Pubkey, account: Account) -> Result<(), LiteSVMError> {
        self.0.set_account(
            address(&key),
            solana_account::Account {
                lamports: account.lamports,
                data: account.data,
                owner: address(&account.owner),
                executable: account.executable,
                rent_epoch: account.rent_epoch,
            },
        )
    }

    pub fn minimum_balance_for_rent_exemption(&self, data_len: usize) -> u64 {
        self.0.minimum_balance_for_rent_exemption(data_len)
    }

    pub fn clock(&self) -> Clock {
        self.0.get_sysvar()
    }

    pub fn set_clock(&mut self, clock: &Clock) {
        self.0.set_sysvar(clock)
    }

    pub fn latest_blockhash(&self) -> Hash {
        Hash::new_from_array(self.0.latest_blockhash().to_bytes())
    }

    pub fn send_transaction(&mut self, tx: Transaction) -> TransactionResult {
        self.0.send_transaction(transaction(tx))
    }
}
