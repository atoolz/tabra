//! Raw PTY management for session mode.
//!
//! Creates a PTY pair (master + slave) and spawns a child shell inside the slave.
//! The master fd is used by the session event loop to read shell output and write
//! forwarded keystrokes.

use anyhow::{Context, Result};
use nix::libc;
use nix::pty::openpty;
use nix::sys::termios;
use nix::unistd::dup;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};

/// A PTY pair: master fd for the session, slave fd for the child shell.
pub struct PtyPair {
    pub master: OwnedFd,
    slave: OwnedFd,
}

impl PtyPair {
    /// Open a new PTY pair.
    pub fn open() -> Result<Self> {
        let result = openpty(None, None).context("openpty failed")?;
        Ok(Self {
            master: result.master,
            slave: result.slave,
        })
    }

    /// Set the PTY window size (propagates to the child shell).
    pub fn set_window_size(&self, rows: u16, cols: u16) -> Result<()> {
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: TIOCSWINSZ is a standard ioctl, master fd is valid.
        let ret = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
        if ret == -1 {
            anyhow::bail!("TIOCSWINSZ failed: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Spawn a child shell process attached to the slave side of the PTY.
    ///
    /// Consumes self, returning the master fd and child process.
    /// The slave fd is dup'd for stdin/stdout/stderr and then closed in the parent,
    /// so that read() on master returns EOF when the child exits.
    pub fn spawn_shell(self, shell: &str, init_script_path: &str) -> Result<(OwnedFd, Child)> {
        // Dup the slave fd for stdout and stderr before consuming the OwnedFd.
        // If either dup fails, self.slave is still owned and will be closed on drop.
        let stdout_fd = dup(self.slave.as_raw_fd()).context("dup slave for stdout")?;
        let stderr_fd = dup(self.slave.as_raw_fd()).context("dup slave for stderr")?;
        // Both dups succeeded: now consume the OwnedFd for stdin
        let slave_raw = self.slave.into_raw_fd();

        let mut cmd = Command::new(shell);

        match shell {
            s if s.ends_with("bash") => {
                cmd.arg("--rcfile").arg(init_script_path);
            }
            s if s.ends_with("fish") => {
                cmd.arg("-C").arg(format!("source {}", init_script_path));
            }
            _ => {
                // zsh and unknown shells: just spawn, integration via env
            }
        }

        // SAFETY: each fd is a distinct open descriptor from openpty/dup.
        // slave_raw is consumed by stdin, stdout_fd by stdout, stderr_fd by stderr.
        // After spawn, the parent has no remaining references to the slave side.
        unsafe {
            cmd.stdin(Stdio::from_raw_fd(slave_raw));
            cmd.stdout(Stdio::from_raw_fd(stdout_fd.into_raw_fd()));
            cmd.stderr(Stdio::from_raw_fd(stderr_fd.into_raw_fd()));
        }

        let child = cmd.spawn().context("failed to spawn shell")?;
        Ok((self.master, child))
    }
}

/// Get the current terminal window size.
pub fn get_window_size() -> Result<(u16, u16)> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ is standard, stdin fd 0 is valid for a terminal.
    let ret = unsafe { libc::ioctl(libc::STDIN_FILENO, libc::TIOCGWINSZ, &mut ws) };
    if ret == -1 {
        anyhow::bail!("TIOCGWINSZ failed: {}", std::io::Error::last_os_error());
    }
    Ok((ws.ws_row, ws.ws_col))
}

/// Put the real terminal stdin into raw mode. Returns the original termios
/// for restoration later.
pub fn enable_raw_mode() -> Result<termios::Termios> {
    let stdin = std::io::stdin();
    let original = termios::tcgetattr(&stdin).context("tcgetattr failed")?;
    let mut raw = original.clone();
    termios::cfmakeraw(&mut raw);
    termios::tcsetattr(&stdin, termios::SetArg::TCSANOW, &raw)
        .context("tcsetattr raw mode failed")?;
    Ok(original)
}

/// Restore terminal to the given termios settings.
pub fn restore_mode(original: &termios::Termios) {
    let stdin = std::io::stdin();
    let _ = termios::tcsetattr(&stdin, termios::SetArg::TCSANOW, original);
}
