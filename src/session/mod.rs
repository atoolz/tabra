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

pub mod buffer_tracker;
pub mod event_loop;
pub mod integration;
pub mod keys;
pub mod osc;
pub mod popup;
pub mod pty;

use anyhow::{Context, Result};
use tabra::shell::ShellType;
use tracing::info;

/// Entry point for `tabra session`.
pub fn run(shell: Option<ShellType>) -> Result<()> {
    let shell_path = match shell {
        Some(ShellType::Bash) => "bash".to_string(),
        Some(ShellType::Zsh) => "zsh".to_string(),
        Some(ShellType::Fish) => "fish".to_string(),
        None => std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string()),
    };

    info!("starting session with shell: {}", shell_path);

    // Ensure daemon is running (silently)
    if !tabra::ipc::client::is_daemon_running() {
        std::process::Command::new("tabra")
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("failed to start tabra daemon")?;

        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if tabra::ipc::client::is_daemon_running() {
                break;
            }
        }
    }

    // Write integration script to a temp file for bash --rcfile
    let integration_script = match shell_path.as_str() {
        s if s.contains("bash") => integration::bash_integration(),
        s if s.contains("zsh") => integration::zsh_integration(),
        s if s.contains("fish") => integration::fish_integration(),
        _ => String::new(),
    };

    let tmp_dir = std::env::temp_dir().join(format!("tabra-session-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;
    let script_path = tmp_dir.join("integration.sh");
    std::fs::write(&script_path, &integration_script)?;
    let script_path_str = script_path.to_string_lossy().to_string();

    // Open PTY and set window size
    let pty_pair = pty::PtyPair::open()?;
    let (rows, cols) = pty::get_window_size()?;
    pty_pair.set_window_size(rows, cols)?;
    info!("PTY opened, terminal size: {}x{}", cols, rows);

    // Spawn shell inside PTY
    let (master, child) = pty_pair.spawn_shell(&shell_path, &script_path_str)?;
    info!("shell spawned: {}", shell_path);

    // Enable raw mode with scopeguard restoration
    let original_termios = pty::enable_raw_mode()?;
    let _raw_guard = scopeguard::guard(original_termios, |t| pty::restore_mode(&t));

    // Run the async event loop
    let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
    rt.block_on(event_loop::run(master, child, cols, rows))?;

    // Guard drops here, restoring terminal
    drop(_raw_guard);

    // Cleanup temp files
    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(())
}
