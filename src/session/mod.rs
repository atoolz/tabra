//! PTY-based session mode for Tabra.
//!
//! `tabra session` wraps the user's shell in a PTY, intercepting all
//! keystrokes before they reach readline/ZLE/fish. This allows Tabra
//! to handle arrow keys, Tab, and Escape without conflicting with the
//! shell's own key bindings.
//!
//! Architecture:
//! ```text
//! Terminal → tabra session (raw stdin) → PTY master → PTY slave → Shell
//!                ↕ popup rendered directly to terminal
//!           tabra daemon (via Unix socket IPC)
//! ```

pub mod integration;
pub mod keys;
pub mod osc;
pub mod popup;
pub mod pty;

use anyhow::Result;
use tabra::shell::ShellType;

/// Entry point for `tabra session`.
pub fn run(shell: Option<ShellType>) -> Result<()> {
    let shell_path = match shell {
        Some(ShellType::Bash) => "bash".to_string(),
        Some(ShellType::Zsh) => "zsh".to_string(),
        Some(ShellType::Fish) => "fish".to_string(),
        None => std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string()),
    };

    tracing::info!("starting session with shell: {}", shell_path);

    // TODO: Phase 5 (event loop) will implement the full session here.
    // For now, verify the foundation compiles and the PTY can be opened.
    let pty_pair = pty::PtyPair::open()?;
    let (rows, cols) = pty::get_window_size()?;
    pty_pair.set_window_size(rows, cols)?;
    tracing::info!("PTY opened, terminal size: {}x{}", cols, rows);

    // Enable raw mode with scopeguard restoration.
    // Guard restores terminal on drop, including on panic.
    // Phase 5 will run the event loop between enable and the implicit drop at end of scope.
    let original_termios = pty::enable_raw_mode()?;
    let _raw_guard = scopeguard::guard(original_termios, |t| pty::restore_mode(&t));

    // TODO: Phase 5 event loop goes here (between raw mode enable and guard drop)

    // Explicitly drop guard to restore terminal before printing
    drop(_raw_guard);

    eprintln!("tabra session: PTY wrapper mode is under development.");
    eprintln!("For now, use: eval \"$(tabra init bash)\"");

    Ok(())
}
