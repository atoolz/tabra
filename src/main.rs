// Spec types and loader define the full withfig schema. Many fields and methods
// are not yet consumed but will be as generators, bash/fish hooks, and other
// features are added. Suppress dead_code warnings for now.
#![allow(dead_code)]

mod daemon;
mod engine;
mod ipc;
mod render;
mod shell;
mod spec;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "tabra", version, about = "Tab. Complete. Ship.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the tabra daemon (runs in background)
    Daemon {
        /// Path to specs directory (default: ~/.local/share/tabra/specs)
        #[arg(long)]
        specs_dir: Option<PathBuf>,
    },

    /// Print shell hook to stdout (eval in your .zshrc)
    Init {
        /// Shell to generate hook for
        #[arg(value_enum)]
        shell: shell::ShellType,
    },

    /// Send a completion request (called by the shell hook, not by humans)
    Complete {
        /// The full command line buffer
        #[arg(long)]
        buffer: String,

        /// Cursor position (character index, not byte offset) within the buffer
        #[arg(long)]
        cursor: usize,

        /// Current working directory
        #[arg(long)]
        cwd: String,
    },

    /// Accept a suggestion (insert it, called by shell hook)
    Accept {
        /// The suggestion text to insert
        #[arg(long)]
        text: String,
    },

    /// Dismiss the popup (called by shell hook)
    Dismiss,

    /// Check daemon health
    Status,

    /// Stop the running daemon
    Stop,

    /// Install bundled specs from withfig/autocomplete
    InstallSpecs {
        /// Path to a directory of compiled JSON specs
        #[arg(long)]
        from: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tabra=info".into()),
        )
        .init();

    match cli.command {
        Commands::Daemon { specs_dir } => daemon::run(specs_dir),
        Commands::Init { shell } => shell::print_hook(shell),
        Commands::Complete {
            buffer,
            cursor,
            cwd,
        } => ipc::client::request_complete(&buffer, cursor, &cwd),
        Commands::Accept { text } => ipc::client::request_accept(&text),
        Commands::Dismiss => ipc::client::request_dismiss(),
        Commands::Status => ipc::client::request_status(),
        Commands::Stop => ipc::client::request_stop(),
        Commands::InstallSpecs { from } => spec::loader::install_specs(&from),
    }
}
