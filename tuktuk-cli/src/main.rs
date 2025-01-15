use clap::{Parser, Subcommand};
use tuktuk_cli::{
    cmd::{cron, cron_transaction, task, task_queue, tuktuk_config, Opts},
    result::Result,
};

#[derive(Debug, Parser)]
#[command(name = "tuktuk-cli")]
#[command(about = "A Tuktuk CLI tool")]
struct Cli {
    #[command(flatten)]
    opts: Opts,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    TuktukConfig(tuktuk_config::TuktukConfigCmd),
    Task(task::TaskCmd),
    TaskQueue(task_queue::TaskQueueCmd),
    Cron(cron::CronCmd),
    CronTransaction(cron_transaction::CronTransactionCmd),
}

#[tokio::main]
async fn main() -> Result {
    let cli = Cli::parse();
    run(cli).await
}

async fn run(cli: Cli) -> Result {
    match cli.cmd {
        Cmd::TuktukConfig(cmd) => cmd.run(cli.opts).await,
        Cmd::Task(cmd) => cmd.run(cli.opts).await,
        Cmd::TaskQueue(cmd) => cmd.run(cli.opts).await,
        Cmd::Cron(cmd) => cmd.run(cli.opts).await,
        Cmd::CronTransaction(cmd) => cmd.run(cli.opts).await,
    }
}
