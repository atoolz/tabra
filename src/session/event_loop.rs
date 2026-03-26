//! Session event loop: the core of the PTY wrapper.
//!
//! Runs three concurrent async tasks:
//! 1. **stdin reader**: raw bytes from real terminal → KeyEvent → PopupState
//! 2. **PTY reader**: bytes from PTY master → OscParser → forward to real terminal
//! 3. **SIGWINCH handler**: terminal resize → PTY window size update
//!
//! The event loop owns the real terminal output and writes popup ANSI
//! directly to it, separate from the PTY output stream.

use super::buffer_tracker::BufferTracker;
use super::keys::{self, KeyEvent};
use super::osc::{OscEvent, OscParser};
use super::popup::{PopupAction, PopupState};
use crate::session::pty;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::Child;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

/// Actions the writer task should execute on the real terminal.
enum TerminalWrite {
    /// Write bytes to the real terminal stdout (PTY output passthrough).
    Passthrough(Vec<u8>),
    /// Write popup ANSI string to the real terminal.
    ShowPopup(String),
    /// Erase popup (N item lines + 2 border lines).
    ErasePopup(usize),
    /// Erase old popup then show new one (atomic redraw).
    EraseAndShow(usize, String),
    /// Show inline ghost text (dim suggestion after cursor, cleared on next keystroke).
    GhostText(String),
    /// Clear ghost text.
    ClearGhost(usize),
}

/// Run the session event loop. This is the main entry point after PTY setup.
///
/// Takes ownership of the PTY master fd and child process.
/// Runs until the child shell exits or stdin reaches EOF.
pub async fn run(
    master: OwnedFd,
    mut child: Child,
    terminal_cols: u16,
    _terminal_rows: u16,
) -> anyhow::Result<()> {
    let master_fd = master.as_raw_fd();

    // Channel for terminal writes (popup + PTY passthrough)
    let (write_tx, mut write_rx) = mpsc::channel::<TerminalWrite>(256);

    // Popup state
    let popup = std::sync::Arc::new(tokio::sync::Mutex::new(PopupState::new(terminal_cols)));

    // Use blocking I/O for PTY master in a dedicated thread.
    // tokio's AsyncFd requires the fd to be non-blocking, which can cause
    // issues with PTY masters on some platforms. Blocking read in a thread is simpler.
    let master_read_fd = master_fd;
    let (pty_output_tx, mut pty_output_rx) = mpsc::channel::<Vec<u8>>(256);
    let (pty_write_tx, mut pty_write_rx) = mpsc::channel::<Vec<u8>>(256);

    // PTY reader thread (blocking)
    let pty_read_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut file = unsafe { std::fs::File::from_raw_fd(master_read_fd) };
        let mut buf = [0u8; 4096];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if pty_output_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    // EIO is normal when child exits
                    if e.raw_os_error() == Some(5) {
                        break;
                    }
                    error!("PTY read error: {e}");
                    break;
                }
            }
        }
        // Don't close the fd here (it's shared with the write side)
        std::mem::forget(file);
        debug!("PTY read thread exited");
    });

    // PTY writer task (uses blocking write from a tokio task)
    let pty_writer_task = tokio::spawn(async move {
        use std::io::Write;
        // SAFETY: master_fd is valid and shared between read thread and this task.
        // The read thread only reads; this task only writes. No concurrent same-op.
        let mut file = unsafe { std::fs::File::from_raw_fd(master_fd) };
        while let Some(bytes) = pty_write_rx.recv().await {
            if file.write_all(&bytes).is_err() {
                break;
            }
            let _ = file.flush();
        }
        std::mem::forget(file); // don't double-close
        debug!("PTY writer task exited");
    });

    // Debounce channel: buffer updates are debounced (30ms) before triggering popup.
    let (debounce_tx, mut debounce_rx) = mpsc::channel::<(String, usize)>(16);

    // Buffer tracker: tracks what the user types locally (no shell integration needed)
    let tracker = std::sync::Arc::new(tokio::sync::Mutex::new(BufferTracker::new()));

    // Task 1: Read raw stdin and route key events
    let stdin_popup = popup.clone();
    let stdin_write_tx = write_tx.clone();
    let stdin_pty_tx = pty_write_tx.clone();
    let stdin_tracker = tracker.clone();
    let stdin_debounce_tx = debounce_tx.clone();
    let stdin_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 256];
        let mut escape_pending = false;

        loop {
            let n = if escape_pending {
                // Wait briefly for escape sequence continuation
                match tokio::time::timeout(
                    std::time::Duration::from_millis(10),
                    stdin.read(&mut buf),
                )
                .await
                {
                    Ok(Ok(0)) => break,
                    Ok(Ok(n)) => n,
                    Ok(Err(_)) => break,
                    Err(_) => {
                        // Timeout: standalone Escape
                        let (events, pending) = keys::parse_bytes(&[], true);
                        escape_pending = pending;
                        for event in events {
                            handle_key_event(
                                event,
                                &stdin_popup,
                                &stdin_write_tx,
                                &stdin_pty_tx,
                                &stdin_tracker,
                                &stdin_debounce_tx,
                            )
                            .await;
                        }
                        continue;
                    }
                }
            } else {
                match stdin.read(&mut buf).await {
                    Ok(0) => break, // EOF
                    Ok(n) => n,
                    Err(_) => break,
                }
            };

            let (events, pending) = keys::parse_bytes(&buf[..n], escape_pending);
            escape_pending = pending;

            for event in events {
                handle_key_event(
                    event,
                    &stdin_popup,
                    &stdin_write_tx,
                    &stdin_pty_tx,
                    &stdin_tracker,
                    &stdin_debounce_tx,
                )
                .await;
            }
        }
        debug!("stdin reader exited");
    });

    // Debounce task: waits 30ms after last buffer update before triggering popup
    let debounce_popup = popup.clone();
    let debounce_write_tx = write_tx.clone();
    let debounce_task = tokio::spawn(async move {
        let cwd = std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let debounce_dur = std::time::Duration::from_millis(30);

        loop {
            let (mut buffer, mut cursor) = match debounce_rx.recv().await {
                Some(ev) => ev,
                None => break,
            };
            // Drain pending events, keep only the latest
            while let Ok(Some((b, c))) =
                tokio::time::timeout(debounce_dur, debounce_rx.recv()).await
            {
                buffer = b;
                cursor = c;
            }
            let mut p = debounce_popup.lock().await;

            // Clear previous ghost text
            if p.ghost_len > 0 {
                let _ = debounce_write_tx
                    .send(TerminalWrite::ClearGhost(p.ghost_len))
                    .await;
                p.ghost_len = 0;
            }

            let action = p.on_command_line(buffer.clone(), cursor, &cwd).await;
            dispatch_popup_action(action, &debounce_write_tx).await;

            // Show ghost text: the remaining part of the first suggestion
            if !p.items.is_empty() {
                let first_insert = p.items[0].insert.clone();
                let token_start = p.find_token_start();
                let end = cursor.min(buffer.len());
                if token_start <= end {
                    let current_token = &buffer[token_start..end];
                    if first_insert.starts_with(current_token)
                        && first_insert.len() > current_token.len()
                    {
                        let ghost = &first_insert[current_token.len()..];
                        p.ghost_len = ghost.len();
                        let _ = debounce_write_tx
                            .send(TerminalWrite::GhostText(ghost.to_string()))
                            .await;
                    }
                }
            }
        }
    });

    // Task 2: Process PTY output from read thread, strip OSC, forward to terminal
    let pty_popup = popup.clone();
    let pty_terminal_tx = write_tx.clone();
    let tracker_for_osc = tracker.clone();
    let pty_task = tokio::spawn(async move {
        let mut osc_parser = OscParser::new();

        while let Some(chunk) = pty_output_rx.recv().await {
            let (passthrough, events) = osc_parser.feed(&chunk);

            if !passthrough.is_empty() {
                let _ = pty_terminal_tx
                    .send(TerminalWrite::Passthrough(passthrough))
                    .await;
            }

            for event in events {
                match event {
                    OscEvent::CommandLine { buffer, cursor } => {
                        // Sync local buffer tracker with shell's actual state
                        // (corrects any desync from untracked editing operations)
                        let mut t = tracker_for_osc.lock().await;
                        t.sync(buffer, cursor);
                    }
                    OscEvent::PromptStart => {
                        // Reset buffer tracker on new prompt
                        let mut t = tracker_for_osc.lock().await;
                        t.sync(String::new(), 0);
                        drop(t);

                        let mut popup = pty_popup.lock().await;
                        if popup.visible {
                            let lines = popup.items.len().min(10);
                            popup.visible = false;
                            popup.items.clear();
                            popup.popup_lines = 0;
                            let _ = pty_terminal_tx.send(TerminalWrite::ErasePopup(lines)).await;
                        }
                    }
                    OscEvent::PromptEnd => {}
                }
            }
        }
        debug!("PTY processor exited");
    });

    // Task 3: Terminal writer (blocking thread, owns stdout)
    let writer_thread = std::thread::spawn(move || {
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();

        while let Some(action) = write_rx.blocking_recv() {
            match action {
                TerminalWrite::Passthrough(bytes) => {
                    let _ = stdout.write_all(&bytes);
                    let _ = stdout.flush();
                }
                TerminalWrite::ShowPopup(ansi) => {
                    // The rendered string already contains \x1b[s / \x1b[u
                    let _ = stdout.write_all(ansi.as_bytes());
                    let _ = stdout.flush();
                }
                TerminalWrite::ErasePopup(lines) => {
                    use tabra::render::overlay;
                    let erase = overlay::erase_popup(lines);
                    let _ = stdout.write_all(erase.as_bytes());
                    let _ = stdout.flush();
                }
                TerminalWrite::EraseAndShow(erase_lines, show_ansi) => {
                    use tabra::render::overlay;
                    // Erase old popup first
                    let erase = overlay::erase_popup(erase_lines);
                    let _ = stdout.write_all(erase.as_bytes());
                    // Then show new popup
                    let _ = stdout.write_all(b"\x1b[s");
                    let _ = stdout.write_all(show_ansi.as_bytes());
                    let _ = stdout.write_all(b"\x1b[u");
                    let _ = stdout.flush();
                }
                TerminalWrite::GhostText(text) => {
                    // Save cursor, write dim text, restore cursor
                    // The ghost text appears after the cursor in dim gray
                    let _ = stdout.write_all(b"\x1b[s\x1b[90m");
                    let _ = stdout.write_all(text.as_bytes());
                    let _ = stdout.write_all(b"\x1b[0m\x1b[u");
                    let _ = stdout.flush();
                }
                TerminalWrite::ClearGhost(len) => {
                    // Save cursor, overwrite ghost text with spaces, restore
                    let _ = stdout.write_all(b"\x1b[s");
                    let spaces = " ".repeat(len);
                    let _ = stdout.write_all(spaces.as_bytes());
                    let _ = stdout.write_all(b"\x1b[u");
                    let _ = stdout.flush();
                }
            }
        }
        debug!("terminal writer exited");
    });

    // Task 4: SIGWINCH handler
    let sigwinch_popup = popup.clone();
    let sigwinch_task = tokio::spawn(async move {
        let mut signal =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change()) {
                Ok(s) => s,
                Err(e) => {
                    warn!("failed to register SIGWINCH handler: {e}");
                    return;
                }
            };

        loop {
            signal.recv().await;
            if let Ok((rows, cols)) = pty::get_window_size() {
                // Resize PTY
                let ws = nix::libc::winsize {
                    ws_row: rows,
                    ws_col: cols,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                unsafe {
                    nix::libc::ioctl(master_fd, nix::libc::TIOCSWINSZ, &ws);
                }
                // Update popup cols
                sigwinch_popup.lock().await.terminal_cols = cols;
            }
        }
    });

    // Wait for PTY processor to exit (child shell exited)
    let _ = pty_task.await;

    // Clean up: abort tasks and await them to ensure they've stopped
    // before closing the master fd.
    stdin_task.abort();
    let _ = stdin_task.await;

    sigwinch_task.abort();
    let _ = sigwinch_task.await;

    debounce_task.abort();
    let _ = debounce_task.await;

    pty_writer_task.abort();
    let _ = pty_writer_task.await;

    drop(write_tx); // close writer channel so writer thread exits
    let _ = writer_thread.join();
    let _ = pty_read_thread.join();

    // Wait for child process
    let _ = child.wait();

    // The master fd was shared between read thread and writer task via from_raw_fd.
    // Both threads use mem::forget on their File to avoid closing the fd.
    // The OwnedFd `master` is the sole owner and its drop here is the single close.
    // (Do NOT mem::forget master: it must close so the PTY is properly cleaned up.)

    Ok(())
}

/// Handle a key event from stdin: route to popup or forward to PTY.
async fn handle_key_event(
    event: KeyEvent,
    popup: &std::sync::Arc<tokio::sync::Mutex<PopupState>>,
    write_tx: &mpsc::Sender<TerminalWrite>,
    pty_tx: &mpsc::Sender<Vec<u8>>,
    tracker: &std::sync::Arc<tokio::sync::Mutex<BufferTracker>>,
    debounce_tx: &mpsc::Sender<(String, usize)>,
) {
    // ArrowRight with ghost text: accept ghost text by typing it into the shell
    if event == KeyEvent::ArrowRight {
        let mut p = popup.lock().await;
        if p.ghost_len > 0 && !p.items.is_empty() {
            let first_insert = p.items[0].insert.clone();
            let current_token_start = p.find_token_start();
            let current_token =
                &p.last_buffer[current_token_start..p.last_cursor.min(p.last_buffer.len())];
            if let Some(remaining) = first_insert.strip_prefix(current_token) {
                // Clear ghost text
                let _ = write_tx.send(TerminalWrite::ClearGhost(p.ghost_len)).await;
                p.ghost_len = 0;
                // Type the remaining text into the shell
                let mut inject = remaining.as_bytes().to_vec();
                inject.push(b' '); // add space after completion
                let _ = pty_tx.send(inject).await;
                // Hide popup
                if p.visible {
                    let lines = p.popup_lines;
                    p.visible = false;
                    p.items.clear();
                    p.popup_lines = 0;
                    let _ = write_tx.send(TerminalWrite::ErasePopup(lines)).await;
                }
                return;
            }
        }
        drop(p);
    }

    // Clear ghost text on any keystroke (it'll be re-rendered after debounce)
    {
        let mut p = popup.lock().await;
        if p.ghost_len > 0 {
            let _ = write_tx.send(TerminalWrite::ClearGhost(p.ghost_len)).await;
            p.ghost_len = 0;
        }
    }
    let action = popup.lock().await.on_key(&event);
    match action {
        PopupAction::ForwardKey(bytes) => {
            let _ = pty_tx.send(bytes).await;
            // Update local buffer tracker and trigger debounced completion
            let mut t = tracker.lock().await;
            if t.on_key(&event) {
                let _ = debounce_tx.send((t.buffer.clone(), t.cursor)).await;
            }
        }
        PopupAction::Show(ansi) => {
            let _ = write_tx.send(TerminalWrite::ShowPopup(ansi)).await;
        }
        PopupAction::Erase { lines } => {
            let _ = write_tx.send(TerminalWrite::ErasePopup(lines)).await;
        }
        PopupAction::Accept {
            token_start,
            insert_text,
        } => {
            // Erase popup first
            let lines = popup.lock().await.items.len().min(10);
            let _ = write_tx.send(TerminalWrite::ErasePopup(lines)).await;

            // Insert text into the shell by sending Ctrl-A (go to start),
            // Ctrl-K (kill to end), then the full replacement line.
            // This works because readline processes these control chars.
            let popup_guard = popup.lock().await;
            let before = &popup_guard.last_buffer[..token_start];
            let after_cursor = if popup_guard.last_cursor < popup_guard.last_buffer.len() {
                &popup_guard.last_buffer[popup_guard.last_cursor..]
            } else {
                ""
            };
            let new_line = format!("{before}{insert_text} {after_cursor}");
            drop(popup_guard);

            // Ctrl-A (beginning of line) + Ctrl-K (kill line) + retype
            let mut inject = vec![0x01u8]; // Ctrl-A
            inject.push(0x0b); // Ctrl-K
            inject.extend_from_slice(new_line.as_bytes());
            let _ = pty_tx.send(inject).await;
        }
        PopupAction::EraseAndForward { lines, bytes } => {
            let _ = write_tx.send(TerminalWrite::ErasePopup(lines)).await;
            let _ = pty_tx.send(bytes).await;
        }
        PopupAction::EraseAndShow { erase_lines, show } => {
            let _ = write_tx
                .send(TerminalWrite::EraseAndShow(erase_lines, show))
                .await;
        }
        PopupAction::Nothing => {}
    }
}

/// Dispatch a PopupAction from the PTY reader (OSC command line event).
async fn dispatch_popup_action(action: PopupAction, write_tx: &mpsc::Sender<TerminalWrite>) {
    match action {
        PopupAction::Show(ansi) => {
            let _ = write_tx.send(TerminalWrite::ShowPopup(ansi)).await;
        }
        PopupAction::Erase { lines } => {
            let _ = write_tx.send(TerminalWrite::ErasePopup(lines)).await;
        }
        PopupAction::EraseAndShow { erase_lines, show } => {
            let _ = write_tx
                .send(TerminalWrite::EraseAndShow(erase_lines, show))
                .await;
        }
        PopupAction::Nothing => {}
        _ => {} // Other actions not expected from on_command_line
    }
}
