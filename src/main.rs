use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod cli;
mod config;
mod cron_engine;
mod daemon;
mod lookup;
mod paths;
mod persist;
mod pid;
mod preflight;
mod process;
mod retry;
mod signal;
mod store;

#[derive(Parser)]
#[command(name = "runner", version, about = "Thin local agent-run daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Manage the configured target repo
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    /// Run a task now, synchronously, against the configured repo
    Run { task: String },
    /// Show recent runs (alias: ps)
    #[command(alias = "ps")]
    Status {
        /// Only show currently-running rows
        #[arg(long)]
        running: bool,
    },
    /// Show the full result/failure detail and continuation signal for one run
    Logs { run_id: String },
    /// Manage cron-style schedules
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon (detaches into the background)
    Start {
        /// Set the target repo before starting (equivalent to `runner repo set` first)
        #[arg(long)]
        repo: Option<PathBuf>,
    },
    /// Stop the running daemon
    Stop,
    /// Report whether the daemon is running
    Status,
}

#[derive(Subcommand)]
enum RepoAction {
    /// Set the target repo (must contain agent_docs/AGENT.md)
    Set { path: PathBuf },
    /// Show the currently configured target repo
    Show,
}

#[derive(Subcommand)]
enum CronAction {
    /// Add a schedule (standard 5-field cron expression)
    Add { cron_expr: String, task: String },
    /// List all schedules
    List,
    /// Remove a schedule by id
    Remove { schedule_id: String },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Daemon { action } => match action {
            DaemonAction::Start { repo } => cli::daemon::start(repo),
            DaemonAction::Stop => cli::daemon::stop(),
            DaemonAction::Status => cli::daemon::status(),
        },
        Commands::Repo { action } => match action {
            RepoAction::Set { path } => cli::repo::set(&path),
            RepoAction::Show => cli::repo::show(),
        },
        Commands::Run { task } => cli::run::run(&task),
        Commands::Status { running } => cli::status::status(running),
        Commands::Logs { run_id } => cli::logs::logs(&run_id),
        Commands::Cron { action } => match action {
            CronAction::Add { cron_expr, task } => cli::cron::add(&cron_expr, &task),
            CronAction::List => cli::cron::list(),
            CronAction::Remove { schedule_id } => cli::cron::remove(&schedule_id),
        },
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
