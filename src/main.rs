use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod claude;
mod cli;
mod config;
mod cron_engine;
mod daemon;
mod paths;
mod persist;
mod pid;
mod retry;
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
    /// Show or set the configured target repo (setting requires
    /// agent_docs/AGENT.md at the path)
    Repo { path: Option<PathBuf> },
    /// Run a task now, chaining turns (PM scopes, Engineer implements, ...)
    /// until the agent reports it's done
    Run {
        task: String,
        /// Perform exactly one turn instead of chaining
        #[arg(long)]
        once: bool,
    },
    /// Show recent runs (alias: ps)
    #[command(alias = "ps")]
    Status {
        /// Only show currently-running rows
        #[arg(long)]
        running: bool,
    },
    /// Show one run's full detail, or launch the interactive log browser
    /// with no run id
    Logs { run_id: Option<String> },
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
        /// Set the target repo before starting (equivalent to `runner repo <path>` first)
        #[arg(long)]
        repo: Option<PathBuf>,
    },
    /// Stop the running daemon
    Stop,
    /// Report whether the daemon is running
    Status,
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
        Commands::Repo { path } => match path {
            Some(path) => cli::repo::set(&path),
            None => cli::repo::show(),
        },
        Commands::Run { task, once } => cli::run::run(&task, once),
        Commands::Status { running } => cli::status::status(running),
        Commands::Logs { run_id } => match run_id {
            Some(id) => cli::logs::logs(&id),
            None => cli::tui::run(),
        },
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
