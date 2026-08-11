//! `run_task_v0` driven against the built program in an in-process SVM.
//!
//! These cover behaviour the TypeScript suite cannot reach: what a crank turner can hand in,
//! and what the program does with a task returned by the program it just ran. The turner and
//! the returning program are both inputs nobody else chooses, so each is exercised directly
//! rather than through a well-behaved client.
//!
//! Requires the program to be built first: `anchor build` in `solana-programs/`, or set
//! `TUKTUK_SO` to a specific artifact.

use anchor_lang::{AccountDeserialize, AccountSerialize, InstructionData, ToAccountMetas};
use litesvm::{
    types::{FailedTransactionMetadata, TransactionMetadata},
    LiteSVM,
};
use solana_sdk::{
    account::Account,
    clock::Clock,
    instruction::{AccountMeta, Instruction, InstructionError},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program, sysvar,
    transaction::{Transaction, TransactionError},
};
use tuktuk::state::{
    CompiledInstructionV0, CompiledTransactionV0, TransactionSourceV0, TriggerV0, TuktukConfigV0,
};

type SendResult = Result<TransactionMetadata, FailedTransactionMetadata>;

fn so_path() -> String {
    std::env::var("TUKTUK_SO").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../solana-programs/target/deploy/tuktuk.so"
        )
        .to_string()
    })
}

fn config_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"tuktuk_config"], &tuktuk::ID)
}

fn task_queue_pda(config: &Pubkey, id: u32) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"task_queue", config.as_ref(), &id.to_le_bytes()],
        &tuktuk::ID,
    )
}

fn name_mapping_pda(config: &Pubkey, name: &str) -> (Pubkey, u8) {
    let hashed = solana_sdk::hash::hash(name.as_bytes()).to_bytes();
    Pubkey::find_program_address(
        &[b"task_queue_name_mapping", config.as_ref(), &hashed],
        &tuktuk::ID,
    )
}

fn queue_authority_pda(task_queue: &Pubkey, authority: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            b"task_queue_authority",
            task_queue.as_ref(),
            authority.as_ref(),
        ],
        &tuktuk::ID,
    )
}

fn task_pda(task_queue: &Pubkey, id: u16) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"task", task_queue.as_ref(), &id.to_le_bytes()],
        &tuktuk::ID,
    )
}

fn lamports(svm: &LiteSVM, key: &Pubkey) -> u64 {
    svm.get_account(key).map(|a| a.lamports).unwrap_or(0)
}

fn task_account_exists(svm: &LiteSVM, key: &Pubkey) -> bool {
    svm.get_account(key)
        .map(|a| a.owner == tuktuk::ID && !a.data.is_empty())
        .unwrap_or(false)
}

/// The program's own error code, when the failure was one the program named. Asserting on this
/// rather than on "it failed" is what ties a test to the specific refusal it is about -- a run
/// can fail for plenty of reasons that have nothing to do with the guard under test.
fn refusal(result: &SendResult) -> Option<u32> {
    match result {
        Err(e) => match e.err {
            TransactionError::InstructionError(_, InstructionError::Custom(code)) => Some(code),
            _ => None,
        },
        Ok(_) => None,
    }
}

fn code(error: tuktuk::error::ErrorCode) -> Option<u32> {
    Some(u32::from(error))
}

fn send(
    svm: &mut LiteSVM,
    ixs: &[Instruction],
    payer: &Keypair,
    signers: &[&Keypair],
) -> SendResult {
    let blockhash = svm.latest_blockhash();
    let tx = Transaction::new_signed_with_payer(ixs, Some(&payer.pubkey()), signers, blockhash);
    svm.send_transaction(tx)
}

/// A single-instruction compiled transaction with every account read-only, laid out as
/// `[named..., program_id]`.
fn compile_readonly(program_id: Pubkey, named: &[Pubkey], data: Vec<u8>) -> CompiledTransactionV0 {
    let mut accounts: Vec<Pubkey> = named.to_vec();
    let program_id_index = accounts.len() as u8;
    accounts.push(program_id);
    CompiledTransactionV0 {
        num_rw_signers: 0,
        num_ro_signers: 0,
        num_rw: 0,
        accounts,
        instructions: vec![CompiledInstructionV0 {
            program_id_index,
            accounts: (0..named.len() as u8).collect(),
            data,
        }],
        signer_seeds: vec![],
    }
}

/// A task that, when run, hands back `tasks`.
fn returns(tasks: Vec<tuktuk::TaskReturnV0>) -> CompiledTransactionV0 {
    let data = tuktuk::instruction::ReturnTasksV0 {
        args: tuktuk::ReturnTasksArgsV0 { tasks },
    }
    .data();
    compile_readonly(tuktuk::ID, &[system_program::ID], data)
}

fn child(crank_reward: Option<u64>) -> tuktuk::TaskReturnV0 {
    tuktuk::TaskReturnV0 {
        trigger: TriggerV0::Now,
        transaction: TransactionSourceV0::CompiledV0(returns(vec![])),
        crank_reward,
        free_tasks: 0,
        description: "child".to_string(),
    }
}

struct Ctx {
    svm: LiteSVM,
    auth: Keypair,
    task_queue: Pubkey,
}

impl Ctx {
    /// A funded turner, which is a different signer from the queue authority on purpose.
    fn turner(&mut self) -> Keypair {
        let turner = Keypair::new();
        self.svm
            .airdrop(&turner.pubkey(), 1_000_000_000)
            .expect("fund the crank turner");
        turner
    }

    fn set_unix_timestamp(&mut self, ts: i64) {
        let mut clock: Clock = self.svm.get_sysvar();
        clock.unix_timestamp = ts;
        self.svm.set_sysvar(&clock);
    }
}

fn setup(capacity: u16, min_crank_reward: u64, stale_task_age: u32) -> Ctx {
    let mut svm = LiteSVM::new();
    let program = std::fs::read(so_path()).unwrap_or_else(|e| {
        panic!(
            "read {} ({e}). Build the programs first: `anchor build` in solana-programs/, \
             or set TUKTUK_SO.",
            so_path()
        )
    });
    svm.add_program(tuktuk::ID, &program);

    let auth = Keypair::new();
    svm.airdrop(&auth.pubkey(), 1_000_000_000_000)
        .expect("fund the queue authority");

    // The config is written directly: initializing it goes through an approver this suite has
    // no reason to model.
    let (config, bump_seed) = config_pda();
    let mut data = Vec::new();
    TuktukConfigV0 {
        min_task_queue_id: 0,
        next_task_queue_id: 0,
        authority: auth.pubkey(),
        min_deposit: 0,
        bump_seed,
    }
    .try_serialize(&mut data)
    .expect("serialize the config");
    let rent = svm.minimum_balance_for_rent_exemption(data.len());
    svm.set_account(
        config,
        Account {
            lamports: rent,
            data,
            owner: tuktuk::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("write the config");

    let name = "q";
    let (task_queue, _) = task_queue_pda(&config, 0);
    let (task_queue_name_mapping, _) = name_mapping_pda(&config, name);
    let init = Instruction {
        program_id: tuktuk::ID,
        accounts: tuktuk::accounts::InitializeTaskQueueV0 {
            payer: auth.pubkey(),
            tuktuk_config: config,
            update_authority: auth.pubkey(),
            task_queue,
            task_queue_name_mapping,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: tuktuk::instruction::InitializeTaskQueueV0 {
            args: tuktuk::InitializeTaskQueueArgsV0 {
                min_crank_reward,
                name: name.to_string(),
                capacity,
                lookup_tables: vec![],
                stale_task_age,
            },
        }
        .data(),
    };
    send(&mut svm, &[init], &auth, &[&auth]).expect("initialize the task queue");

    let (task_queue_authority, _) = queue_authority_pda(&task_queue, &auth.pubkey());
    let add_authority = Instruction {
        program_id: tuktuk::ID,
        accounts: tuktuk::accounts::AddQueueAuthorityV0 {
            payer: auth.pubkey(),
            update_authority: auth.pubkey(),
            queue_authority: auth.pubkey(),
            task_queue_authority,
            task_queue,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: tuktuk::instruction::AddQueueAuthorityV0.data(),
    };
    send(&mut svm, &[add_authority], &auth, &[&auth]).expect("add the queue authority");

    svm.airdrop(&task_queue, 1_000_000_000)
        .expect("fund the task queue");

    Ctx {
        svm,
        auth,
        task_queue,
    }
}

/// Queue a task, returning whatever the queue instruction did so a caller can assert on it.
fn queue(
    ctx: &mut Ctx,
    id: u16,
    trigger: TriggerV0,
    transaction: CompiledTransactionV0,
    free_tasks: u8,
) -> SendResult {
    queue_source(
        ctx,
        id,
        trigger,
        TransactionSourceV0::CompiledV0(transaction),
        free_tasks,
    )
}

/// Queue a task from any transaction source, which a remote task needs.
fn queue_source(
    ctx: &mut Ctx,
    id: u16,
    trigger: TriggerV0,
    transaction: TransactionSourceV0,
    free_tasks: u8,
) -> SendResult {
    let (task, _) = task_pda(&ctx.task_queue, id);
    let (task_queue_authority, _) = queue_authority_pda(&ctx.task_queue, &ctx.auth.pubkey());
    let auth = ctx.auth.insecure_clone();
    let ix = Instruction {
        program_id: tuktuk::ID,
        accounts: tuktuk::accounts::QueueTaskV0 {
            payer: auth.pubkey(),
            queue_authority: auth.pubkey(),
            task_queue_authority,
            task_queue: ctx.task_queue,
            task,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: tuktuk::instruction::QueueTaskV0 {
            args: tuktuk::QueueTaskArgsV0 {
                id,
                trigger,
                transaction,
                crank_reward: None,
                free_tasks,
                description: "t".to_string(),
            },
        }
        .data(),
    };
    send(&mut ctx.svm, &[ix], &auth, &[&auth])
}

/// Run `task` as `turner`, pairing `free_task_ids` with `free_task_accounts`. The two are
/// passed separately because a turner chooses both and they need not agree.
fn run_task(
    ctx: &mut Ctx,
    task_id: u16,
    turner: &Keypair,
    free_task_ids: Vec<u16>,
    free_task_accounts: Vec<Pubkey>,
) -> SendResult {
    let named = vec![
        AccountMeta::new_readonly(system_program::ID, false),
        AccountMeta::new_readonly(tuktuk::ID, false),
    ];
    run_task_named(
        ctx,
        task_id,
        turner,
        named,
        free_task_ids,
        free_task_accounts,
    )
}

/// `run_task` where the caller states the queued transaction's own accounts, which a task
/// naming anything other than `[system_program, tuktuk]` needs.
fn run_task_named(
    ctx: &mut Ctx,
    task_id: u16,
    turner: &Keypair,
    named: Vec<AccountMeta>,
    free_task_ids: Vec<u16>,
    free_task_accounts: Vec<Pubkey>,
) -> SendResult {
    let (task, _) = task_pda(&ctx.task_queue, task_id);
    // The named accounts of the queued transaction, then the free-task accounts.
    let mut metas = tuktuk::accounts::RunTaskV0 {
        crank_turner: turner.pubkey(),
        rent_refund: ctx.auth.pubkey(),
        task_queue: ctx.task_queue,
        task,
        system_program: system_program::ID,
        sysvar_instructions: sysvar::instructions::ID,
    }
    .to_account_metas(None);
    metas.extend(named);
    metas.extend(
        free_task_accounts
            .iter()
            .map(|a| AccountMeta::new(*a, false)),
    );

    let ix = Instruction {
        program_id: tuktuk::ID,
        accounts: metas,
        data: tuktuk::instruction::RunTaskV0 {
            args: tuktuk::RunTaskArgsV0 { free_task_ids },
        }
        .data(),
    };
    let blockhash = ctx.svm.latest_blockhash();
    let tx =
        Transaction::new_signed_with_payer(&[ix], Some(&turner.pubkey()), &[turner], blockhash);
    ctx.svm.send_transaction(tx)
}

/// Queue a task that hands one child back through an account the returning program owns, the
/// child carrying `payload_len` bytes. Returns the accounts that task names.
fn queue_returning_task(
    ctx: &mut Ctx,
    payload_len: u32,
    free_tasks: u8,
    names: Vec<u8>,
) -> Vec<AccountMeta> {
    let artifact = std::path::Path::new(&so_path())
        .parent()
        .expect("the built program has a directory")
        .join("return_example.so");
    let program = std::fs::read(&artifact).unwrap_or_else(|e| {
        panic!("read {artifact:?} ({e}); run `anchor build` in solana-programs/")
    });
    ctx.svm.add_program(return_example::ID, &program);

    let (queue_authority, _) =
        Pubkey::find_program_address(&[b"queue_authority"], &return_example::ID);
    let (task_return_account, _) =
        Pubkey::find_program_address(&[b"task_return_account"], &return_example::ID);
    ctx.svm
        .airdrop(&queue_authority, 10_000_000_000)
        .expect("fund the account the return is written from");

    let transaction = CompiledTransactionV0 {
        num_rw_signers: 0,
        num_ro_signers: 0,
        num_rw: 2,
        accounts: vec![
            queue_authority,
            task_return_account,
            system_program::ID,
            return_example::ID,
        ],
        instructions: vec![CompiledInstructionV0 {
            program_id_index: 3,
            accounts: names,
            data: return_example::instruction::ReturnTaskWithPayload { payload_len }.data(),
        }],
        signer_seeds: vec![],
    };
    queue(ctx, 0, TriggerV0::Now, transaction, free_tasks).expect("queue the parent");

    vec![
        AccountMeta::new(queue_authority, false),
        AccountMeta::new(task_return_account, false),
        AccountMeta::new_readonly(system_program::ID, false),
        AccountMeta::new_readonly(return_example::ID, false),
    ]
}

#[test]
fn a_returned_tasks_account_is_read_once_however_often_it_is_named() {
    let mut ctx = setup(100, 10_000, 100_000);
    let named = queue_returning_task(&mut ctx, 32, 3, vec![0, 1, 2]);

    let turner = ctx.turner();
    let (first, _) = task_pda(&ctx.task_queue, 1);
    let (second, _) = task_pda(&ctx.task_queue, 2);

    // The turner supplies one free task per id, and puts the account the child was returned in
    // where the third free task would go.
    let (returned_in, _) =
        Pubkey::find_program_address(&[b"task_return_account"], &return_example::ID);
    let queue_before = lamports(&ctx.svm, &ctx.task_queue);
    let result = run_task_named(
        &mut ctx,
        0,
        &turner,
        named,
        vec![1, 2, 3],
        vec![first, second, returned_in],
    );
    assert!(
        result.is_ok(),
        "run failed: {:?}",
        result.err().map(|e| e.err)
    );

    assert!(
        task_account_exists(&ctx.svm, &first),
        "the returned child should have been created"
    );
    assert!(
        !task_account_exists(&ctx.svm, &second),
        "one child was returned, so only one should exist"
    );
    // Every task the queue funds costs it a reward it does not get back.
    let spent = queue_before as i64 - lamports(&ctx.svm, &ctx.task_queue) as i64;
    assert!(
        spent <= 10_000,
        "the queue paid {spent} lamports, which is more than one child's reward"
    );
}

/// A tasks account is read out of the accounts the returning instruction was given. The free
/// tasks a run also holds are the crank turner's to choose, so a program naming one of those
/// names an account the turner supplied rather than one of its own.
#[test]
fn a_tasks_account_the_instruction_did_not_name_is_not_read() {
    let mut ctx = setup(100, 10_000, 100_000);
    let turner = ctx.turner();
    let (returned_in, _) =
        Pubkey::find_program_address(&[b"task_return_account"], &return_example::ID);

    // A first run to write the tasks account, which is what the second run names.
    let named = queue_returning_task(&mut ctx, 32, 1, vec![0, 1, 2]);
    let (first_child, _) = task_pda(&ctx.task_queue, 1);
    run_task_named(&mut ctx, 0, &turner, named, vec![1], vec![first_child])
        .expect("run the parent that writes the tasks account");
    assert!(
        task_account_exists(&ctx.svm, &first_child),
        "the first run should have created its child"
    );
    // The run below is about a tasks account holding a task, so this pins that it holds one.
    assert!(
        ctx.svm
            .get_account(&returned_in)
            .is_some_and(|a| a.owner == return_example::ID && !a.data.is_empty()),
        "the first run should have left a task in the tasks account"
    );

    // A second parent whose instruction names only the system program, and whose program hands
    // back the tasks account anyway.
    let transaction = CompiledTransactionV0 {
        num_rw_signers: 0,
        num_ro_signers: 0,
        num_rw: 0,
        accounts: vec![system_program::ID, return_example::ID],
        instructions: vec![CompiledInstructionV0 {
            program_id_index: 1,
            accounts: vec![0],
            data: return_example::instruction::ReturnTasksAccountWithoutNamingIt.data(),
        }],
        signer_seeds: vec![],
    };
    queue(&mut ctx, 3, TriggerV0::Now, transaction, 2).expect("queue the second parent");

    // Two free tasks: the one a child would be created in, and the tasks account behind it. The
    // second is never consumed, since reading it is what this asserts does not happen.
    let (second_child, _) = task_pda(&ctx.task_queue, 4);
    let result = run_task_named(
        &mut ctx,
        3,
        &turner,
        vec![
            AccountMeta::new_readonly(system_program::ID, false),
            AccountMeta::new_readonly(return_example::ID, false),
        ],
        vec![4, 5],
        vec![second_child, returned_in],
    );
    assert!(
        result.is_ok(),
        "run failed: {:?}",
        result.err().map(|e| e.err)
    );

    assert!(
        !task_account_exists(&ctx.svm, &second_child),
        "the tasks account reached this run only as a free-task account, so nothing should have \
         been queued out of it"
    );
}

#[test]
fn a_matching_free_task_account_creates_the_child() {
    let mut ctx = setup(100, 10_000, 100_000);
    let _ = queue(&mut ctx, 0, TriggerV0::Now, returns(vec![child(None)]), 1)
        .expect("queue the parent");

    let turner = ctx.turner();
    let (child_task, _) = task_pda(&ctx.task_queue, 1);
    let result = run_task(&mut ctx, 0, &turner, vec![1], vec![child_task]);

    assert!(
        result.is_ok(),
        "run failed: {:?}",
        result.err().map(|e| e.err)
    );
    assert!(
        task_account_exists(&ctx.svm, &child_task),
        "the returned child should have been created at id 1"
    );
}

#[test]
fn a_free_task_account_that_does_not_match_its_id_fails_the_run() {
    let mut ctx = setup(100, 10_000, 100_000);
    let _ = queue(&mut ctx, 0, TriggerV0::Now, returns(vec![child(None)]), 1)
        .expect("queue the parent");

    let turner = ctx.turner();
    let (intended, _) = task_pda(&ctx.task_queue, 1);
    let (mismatched, _) = task_pda(&ctx.task_queue, 2);

    // The id list and the account list are the same length, so only the pairing is wrong.
    let result = run_task(&mut ctx, 0, &turner, vec![1], vec![mismatched]);

    assert_eq!(
        refusal(&result),
        code(tuktuk::error::ErrorCode::InvalidTaskPDA),
        "expected the mismatched free-task account to be refused by name, got {:?}",
        result.as_ref().err().map(|e| &e.err)
    );
    assert!(
        !task_account_exists(&ctx.svm, &intended) && !task_account_exists(&ctx.svm, &mismatched),
        "no task account should have been created"
    );
}

#[test]
fn fewer_free_task_accounts_than_ids_is_refused() {
    let mut ctx = setup(100, 10_000, 100_000);
    let _ = queue(&mut ctx, 0, TriggerV0::Now, returns(vec![child(None)]), 1)
        .expect("queue the parent");

    let turner = ctx.turner();
    let (child_task, _) = task_pda(&ctx.task_queue, 1);

    // An id to consume, and no account to pair it with.
    let result = run_task(&mut ctx, 0, &turner, vec![1], vec![]);

    // Named, so this pins the count check rather than passing on any failure at all.
    assert_eq!(
        refusal(&result),
        code(tuktuk::error::ErrorCode::MismatchedFreeTaskCounts),
        "expected the account count to be refused, got {:?}",
        result.as_ref().err().map(|e| &e.err)
    );
    assert!(
        !task_account_exists(&ctx.svm, &child_task),
        "no task account should have been created"
    );
}

#[test]
fn free_tasks_just_under_the_u8_limit_is_accepted() {
    let mut ctx = setup(300, 10_000, 100_000);
    let result = queue(&mut ctx, 0, TriggerV0::Now, returns(vec![]), 254);
    assert!(
        result.is_ok(),
        "free_tasks=254 on a capacity-300 queue should be accepted: {:?}",
        result.err().map(|e| e.err)
    );
}

#[test]
fn free_tasks_at_the_u8_limit_is_accepted() {
    // 255 is a legitimate declaration on a queue with room for it, and adding one to it has to
    // happen in a type that can hold the result.
    let mut ctx = setup(300, 10_000, 100_000);
    let result = queue(&mut ctx, 0, TriggerV0::Now, returns(vec![]), 255);
    assert!(
        result.is_ok(),
        "free_tasks=255 on a capacity-300 queue should be accepted: {:?}",
        result.err().map(|e| e.err)
    );
}

#[test]
fn a_returned_reward_above_the_queue_minimum_does_not_spend_the_pool() {
    let min_crank_reward = 10_000u64;
    let mut ctx = setup(100, min_crank_reward, 100_000);
    let inflated = 100_000_000u64;
    let _ = queue(
        &mut ctx,
        0,
        TriggerV0::Now,
        returns(vec![child(Some(inflated))]),
        1,
    )
    .expect("queue the parent");

    let turner = ctx.turner();
    let (child_task, _) = task_pda(&ctx.task_queue, 1);
    let before = lamports(&ctx.svm, &ctx.task_queue);
    let result = run_task(&mut ctx, 0, &turner, vec![1], vec![child_task]);
    assert!(
        result.is_ok(),
        "run failed: {:?}",
        result.err().map(|e| e.err)
    );
    let spent = before as i64 - lamports(&ctx.svm, &ctx.task_queue) as i64;

    assert!(
        !task_account_exists(&ctx.svm, &child_task),
        "a child whose reward exceeds the queue minimum should not be created"
    );
    assert!(
        spent == 0,
        "the queue paid {spent} lamports for a child it did not create \
         (returned reward {inflated}, queue minimum {min_crank_reward})"
    );
}

/// Ask the queue's update authority to set `stale_task_age`.
fn set_stale_task_age(ctx: &Ctx, stale_task_age: u32) -> Instruction {
    Instruction {
        program_id: tuktuk::ID,
        accounts: tuktuk::accounts::UpdateTaskQueueV0 {
            payer: ctx.auth.pubkey(),
            update_authority: ctx.auth.pubkey(),
            task_queue: ctx.task_queue,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: tuktuk::instruction::UpdateTaskQueueV0 {
            args: tuktuk::UpdateTaskQueueArgsV0 {
                min_crank_reward: None,
                capacity: None,
                lookup_tables: None,
                update_authority: None,
                stale_task_age: Some(stale_task_age),
            },
        }
        .data(),
    }
}

#[test]
fn stale_task_age_can_be_lowered_on_an_empty_queue() {
    // Nothing was queued under the old value, so nothing is measured against it.
    let mut ctx = setup(100, 10_000, 1_000_000);
    let auth = ctx.auth.insecure_clone();

    let lower = set_stale_task_age(&ctx, 0);
    let lowered = send(&mut ctx.svm, &[lower], &auth, &[&auth]);

    assert!(
        lowered.is_ok(),
        "an empty queue should take any age: {:?}",
        lowered.as_ref().err().map(|e| &e.err)
    );
}

#[test]
fn stale_task_age_can_be_raised_on_a_queue_holding_tasks() {
    // The floor is the value a task was queued under, so moving away from it stays allowed. A
    // queue that has ever held a task would otherwise be stuck at its first value for good.
    let mut ctx = setup(100, 10_000, 1_000_000);
    ctx.set_unix_timestamp(100_000);

    let _ = queue(
        &mut ctx,
        0,
        TriggerV0::Timestamp(99_900),
        returns(vec![child(None)]),
        1,
    )
    .expect("queue the parent");

    let auth = ctx.auth.insecure_clone();
    let raise = set_stale_task_age(&ctx, 2_000_000);
    let raised = send(&mut ctx.svm, &[raise], &auth, &[&auth]);
    assert!(
        raised.is_ok(),
        "raising stale_task_age on a live queue should be allowed: {:?}",
        raised.as_ref().err().map(|e| &e.err)
    );

    // The stored field, rather than a later run that would pass at either value.
    let account = ctx
        .svm
        .get_account(&ctx.task_queue)
        .expect("the task queue account");
    let queue_acc = tuktuk::state::TaskQueueV0::try_deserialize(&mut account.data.as_slice())
        .expect("deserialize the task queue");
    assert_eq!(queue_acc.stale_task_age, 2_000_000);
}

#[test]
fn stale_task_age_cannot_be_lowered() {
    let mut ctx = setup(100, 10_000, 1_000_000);
    ctx.set_unix_timestamp(100_000);

    // A task already queued against the current age.
    let _ = queue(
        &mut ctx,
        0,
        TriggerV0::Timestamp(99_900),
        returns(vec![child(None)]),
        1,
    )
    .expect("queue the parent");

    let auth = ctx.auth.insecure_clone();
    let lower = set_stale_task_age(&ctx, 0);
    let lowered = send(&mut ctx.svm, &[lower], &auth, &[&auth]);
    assert_eq!(
        refusal(&lowered),
        code(tuktuk::error::ErrorCode::StaleTaskAgeCannotDecrease),
        "expected lowering stale_task_age to be refused by name, got {:?}",
        lowered.as_ref().err().map(|e| &e.err)
    );

    // The queued task still runs and still creates its child.
    let turner = ctx.turner();
    let (child_task, _) = task_pda(&ctx.task_queue, 1);
    let result = run_task(&mut ctx, 0, &turner, vec![1], vec![child_task]);
    assert!(
        result.is_ok(),
        "run failed: {:?}",
        result.err().map(|e| e.err)
    );
    assert!(
        task_account_exists(&ctx.svm, &child_task),
        "the task should have run rather than been treated as stale"
    );
}

#[test]
fn an_account_index_outside_the_provided_accounts_is_refused() {
    let mut ctx = setup(100, 10_000, 1_000_000);
    let transaction = CompiledTransactionV0 {
        num_rw_signers: 0,
        num_ro_signers: 0,
        num_rw: 0,
        accounts: vec![system_program::ID, tuktuk::ID],
        instructions: vec![CompiledInstructionV0 {
            program_id_index: 1,
            // Two accounts are provided, so index 9 names none of them.
            accounts: vec![9],
            data: vec![],
        }],
        signer_seeds: vec![],
    };
    let _ = queue(&mut ctx, 0, TriggerV0::Now, transaction, 0).expect("queue the task");

    let turner = ctx.turner();
    let result = run_task(&mut ctx, 0, &turner, vec![], vec![]);

    assert_eq!(
        refusal(&result),
        code(tuktuk::error::ErrorCode::InvalidAccountIndex),
        "expected the out-of-range index to be refused by name, got {:?}",
        result.as_ref().err().map(|e| &e.err)
    );
}

#[test]
fn a_program_id_index_outside_the_provided_accounts_is_refused() {
    let mut ctx = setup(100, 10_000, 1_000_000);
    let transaction = CompiledTransactionV0 {
        num_rw_signers: 0,
        num_ro_signers: 0,
        num_rw: 0,
        accounts: vec![system_program::ID, tuktuk::ID],
        instructions: vec![CompiledInstructionV0 {
            program_id_index: 9,
            // The accounts resolve, so the run reaches the program the instruction names.
            accounts: vec![0],
            data: vec![],
        }],
        signer_seeds: vec![],
    };
    let _ = queue(&mut ctx, 0, TriggerV0::Now, transaction, 0).expect("queue the task");

    let turner = ctx.turner();
    let result = run_task(&mut ctx, 0, &turner, vec![], vec![]);

    assert_eq!(
        refusal(&result),
        code(tuktuk::error::ErrorCode::InvalidAccountIndex),
        "expected the program id index to be refused by name, got {:?}",
        result.as_ref().err().map(|e| &e.err)
    );
}

#[test]
fn a_remote_task_run_first_in_its_transaction_is_refused() {
    // A remote task is verified against a signature carried by the instruction before it, so
    // there has to be one. The crank turner composes the transaction and chooses the position.
    let mut ctx = setup(100, 10_000, 1_000_000);
    let _ = queue_source(
        &mut ctx,
        0,
        TriggerV0::Now,
        TransactionSourceV0::RemoteV0 {
            url: "https://example.invalid/task".to_string(),
            signer: Pubkey::new_unique(),
        },
        0,
    )
    .expect("queue the task");

    let turner = ctx.turner();
    let result = run_task(&mut ctx, 0, &turner, vec![], vec![]);

    assert_eq!(
        refusal(&result),
        code(tuktuk::error::ErrorCode::MalformedRemoteTransaction),
        "expected a remote task with nothing before it to be refused by name, got {:?}",
        result.as_ref().err().map(|e| &e.err)
    );
}

#[test]
fn a_signer_seed_over_the_length_limit_is_refused() {
    let mut ctx = setup(100, 10_000, 1_000_000);
    let transaction = CompiledTransactionV0 {
        num_rw_signers: 0,
        num_ro_signers: 0,
        num_rw: 0,
        accounts: vec![system_program::ID, tuktuk::ID],
        instructions: vec![CompiledInstructionV0 {
            program_id_index: 1,
            accounts: vec![0],
            data: vec![],
        }],
        // A seed may be at most 32 bytes.
        signer_seeds: vec![vec![vec![0u8; 33]]],
    };
    let _ = queue(&mut ctx, 0, TriggerV0::Now, transaction, 0).expect("queue the task");

    let turner = ctx.turner();
    let result = run_task(&mut ctx, 0, &turner, vec![], vec![]);

    assert_eq!(
        refusal(&result),
        code(tuktuk::error::ErrorCode::InvalidSignerSeeds),
        "expected the over-long seed to be refused by name, got {:?}",
        result.as_ref().err().map(|e| &e.err)
    );
}

#[test]
fn a_returned_tasks_account_named_twice_by_the_program_is_read_once() {
    let mut ctx = setup(100, 10_000, 100_000);
    // The instruction names the account its child is returned in twice. Account indices are a
    // list, so a program may name one account as often as it likes.
    let named = queue_returning_task(&mut ctx, 32, 2, vec![0, 1, 2, 1]);

    let turner = ctx.turner();
    let (first, _) = task_pda(&ctx.task_queue, 1);
    let (second, _) = task_pda(&ctx.task_queue, 2);

    let result = run_task_named(&mut ctx, 0, &turner, named, vec![1, 2], vec![first, second]);

    assert!(
        result.is_ok(),
        "run failed: {:?}",
        result.as_ref().err().map(|e| &e.err)
    );
    assert!(
        task_account_exists(&ctx.svm, &first),
        "the returned child should have been created"
    );
    assert!(
        !task_account_exists(&ctx.svm, &second),
        "one child was returned, so only one should exist"
    );
}

#[test]
fn a_returned_task_of_a_few_kilobytes_is_created() {
    // The size a returned task serializes to is measured, not built, so a task of real size
    // costs its own bytes once rather than several times over.
    let mut ctx = setup(100, 10_000, 100_000);
    let named = queue_returning_task(&mut ctx, 4_000, 1, vec![0, 1, 2]);
    let turner = ctx.turner();
    let (child, _) = task_pda(&ctx.task_queue, 1);

    let result = run_task_named(&mut ctx, 0, &turner, named, vec![1], vec![child]);

    assert!(
        result.is_ok(),
        "run failed: {:?}",
        result.as_ref().err().map(|e| &e.err)
    );
    assert!(
        task_account_exists(&ctx.svm, &child),
        "the returned task should have been created"
    );
}
