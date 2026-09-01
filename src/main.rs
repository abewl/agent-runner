use clap::{Parser, Subcommand};

mod cli;
mod daemon;
mod paths;
mod pid;

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
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon (detaches into the background)
    Start,
    /// Stop the running daemon
    Stop,
    /// Report whether the daemon is running
    Status,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Daemon { action } => match action {
            DaemonAction::Start => cli::daemon::start(),
            DaemonAction::Stop => cli::daemon::stop(),
            DaemonAction::Status => cli::daemon::status(),
        },
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
