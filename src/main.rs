mod daemon;
// Session modules define the PTY wrapper infrastructure. Many types are not
// yet consumed until the event loop (Phase 5) is implemented.
#[allow(dead_code)]
mod session;

use tabra::ipc;
use tabra::shell;
use tabra::spec;

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

    /// Send a completion request, return JSON response (for programmatic clients)
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

        /// Terminal width in columns
        #[arg(long)]
        cols: Option<u16>,
    },

    /// Send a completion request, return shell-friendly output (for shell hooks)
    /// Format: first line = count, then one line per item: display\tinsert\tdescription
    /// If --render is passed, last line is the pre-rendered ANSI popup.
    CompleteShell {
        /// The full command line buffer
        #[arg(long)]
        buffer: String,

        /// Cursor position (character index, not byte offset) within the buffer
        #[arg(long)]
        cursor: usize,

        /// Current working directory
        #[arg(long)]
        cwd: String,

        /// Terminal width in columns
        #[arg(long)]
        cols: Option<u16>,

        /// Include pre-rendered ANSI popup in output (last section after blank line)
        #[arg(long)]
        render: bool,
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

    /// Validate compiled JSON specs against the Tabra type system
    ValidateSpecs {
        /// Path to a directory of JSON specs to validate
        #[arg(long)]
        from: PathBuf,
    },

    /// Start a PTY-wrapped shell session with autocomplete
    /// (Arrow keys, Tab, Escape all work without readline conflicts)
    Session {
        /// Shell to use (defaults to $SHELL)
        #[arg(value_enum)]
        shell: Option<shell::ShellType>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Session mode defaults to warn (no visible logs).
    // Other modes default to info. RUST_LOG overrides both.
    let default_level = match &cli.command {
        Commands::Session { .. } => "tabra=warn",
        _ => "tabra=info",
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_level.into()),
        )
        .init();

    match cli.command {
        Commands::Daemon { specs_dir } => daemon::run(specs_dir),
        Commands::Init { shell } => shell::print_hook(shell),
        Commands::Complete {
            buffer,
            cursor,
            cwd,
            cols,
        } => ipc::client::request_complete(&buffer, cursor, &cwd, cols),
        Commands::CompleteShell {
            buffer,
            cursor,
            cwd,
            cols,
            render,
        } => ipc::client::request_complete_shell(&buffer, cursor, &cwd, cols, render),
        Commands::Accept { text } => ipc::client::request_accept(&text),
        Commands::Dismiss => ipc::client::request_dismiss(),
        Commands::Status => ipc::client::request_status(),
        Commands::Stop => ipc::client::request_stop(),
        Commands::InstallSpecs { from } => spec::loader::install_specs(&from),
        Commands::ValidateSpecs { from } => spec::loader::validate_specs(&from),
        Commands::Session { shell } => session::run(shell),
    }
}
