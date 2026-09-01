use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod cli;
mod config;
mod daemon;
mod paths;
mod pid;
// Not yet called from a CLI command — F-10 (`runner run`), the intended
// caller, doesn't exist yet. Genuinely unused for now, not dead code; the
// allow comes off once F-10 wires it in.
#[allow(dead_code)]
mod lookup;
#[allow(dead_code)]
mod persist;
#[allow(dead_code)]
mod preflight;
#[allow(dead_code)]
mod process;
#[allow(dead_code)]
mod retry;
#[allow(dead_code)]
mod signal;
#[allow(dead_code)]
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
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
