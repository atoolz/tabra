//! IPC protocol between shell hook (client) and daemon (server).
//!
//! Communication happens over a Unix domain socket at:
//!   $XDG_RUNTIME_DIR/tabra.sock  (or /tmp/tabra-$UID.sock as fallback)
//!
//! Protocol: newline-delimited JSON messages (JSON-Lines).
//! Each message is a single JSON object followed by '\n'.
//!
//! Request flow:
//!   Shell hook sends a Request, daemon replies with a Response.
//!   The connection is short-lived: one request, one response, then close.

use crate::spec::types::SuggestionType;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Socket path
// ---------------------------------------------------------------------------

/// Get the Unix socket path for the daemon.
pub fn socket_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("tabra.sock")
    } else {
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/tabra-{uid}.sock"))
    }
}

// ---------------------------------------------------------------------------
// Request (shell hook -> daemon)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Request completions for the current command line.
    Complete {
        /// The full command line buffer.
        buffer: String,
        /// Cursor position as a character index (not byte offset).
        /// ZLE's $CURSOR is a character index; the parser converts to bytes internally.
        cursor: usize,
        /// Current working directory.
        cwd: String,
    },

    /// User accepted a suggestion from the popup.
    Accept {
        /// The text to insert.
        text: String,
    },

    /// User dismissed the popup (Escape, Ctrl-C, etc).
    Dismiss,

    /// Health check.
    Status,

    /// Shutdown the daemon.
    Stop,
}

// ---------------------------------------------------------------------------
// Response (daemon -> shell hook)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Completion results to display in the popup.
    Completions {
        /// Suggestions to show (already ranked).
        items: Vec<CompletionItem>,
        /// Index of the selected item (0 = first).
        selected: usize,
        /// The partial token these suggestions are filtering on.
        query: String,
    },

    /// No completions available (hide popup if visible).
    Empty,

    /// Acknowledgement (for Accept, Dismiss).
    Ack,

    /// Daemon status information.
    StatusInfo {
        /// Number of loaded specs.
        specs_loaded: usize,
        /// Daemon uptime in seconds.
        uptime_secs: u64,
        /// Daemon PID.
        pid: u32,
    },

    /// Daemon is shutting down.
    Goodbye,

    /// Error message.
    Error { message: String },
}

// ---------------------------------------------------------------------------
// CompletionItem (what gets sent to the shell hook for rendering)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    /// Text displayed in the popup.
    pub display: String,

    /// Text inserted on accept.
    pub insert: String,

    /// Description/help text.
    pub description: String,

    /// Kind (for icon rendering).
    pub kind: SuggestionType,

    /// Character indices (not byte offsets) indicating which characters matched the query.
    /// Produced by nucleo against `match_text`; applied to `display` by the overlay renderer.
    pub match_indices: Vec<u32>,

    /// Whether this is dangerous.
    pub is_dangerous: bool,
}

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

impl Request {
    pub fn to_json_line(&self) -> String {
        let mut s = serde_json::to_string(self).expect("Request serialization");
        s.push('\n');
        s
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s.trim())
    }
}

impl Response {
    pub fn to_json_line(&self) -> String {
        let mut s = serde_json::to_string(self).expect("Response serialization");
        s.push('\n');
        s
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s.trim())
    }
}
