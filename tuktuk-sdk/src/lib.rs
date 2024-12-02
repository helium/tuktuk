pub mod client;
pub mod clock;
pub mod compiled_transaction;
pub mod error;
pub mod instruction;
pub mod tuktuk;
pub mod watcher;

pub mod prelude {
    pub use anchor_lang::prelude::*;

    pub use crate::{
        client::{GetAccount, GetAnchorAccount},
        clock, program, tuktuk, watcher,
    };
}
