//! IPC client used by the CLI subcommands (complete, accept, dismiss, etc.)
//! to talk to the running daemon.

use crate::ipc::protocol::{self, Request, Response};
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const READ_TIMEOUT: Duration = Duration::from_secs(2);

fn send_request(request: &Request) -> Result<Response> {
    let socket_path = protocol::socket_path();

    let stream = UnixStream::connect_addr(
        &std::os::unix::net::SocketAddr::from_pathname(&socket_path)
            .context("invalid socket path")?,
    )
    .or_else(|_| UnixStream::connect(&socket_path))
    .context("failed to connect to tabra daemon (is it running?)")?;

    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .context("set read timeout")?;
    stream
        .set_write_timeout(Some(CONNECT_TIMEOUT))
        .context("set write timeout")?;

    let mut stream = stream;
    let request_line = request.to_json_line();
    stream
        .write_all(request_line.as_bytes())
        .context("write request")?;
    stream.flush().context("flush request")?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).context("read response")?;

    Response::from_json(&line).context("parse response")
}

/// Request completions and print the response JSON to stdout.
/// Used by programmatic clients and tests.
pub fn request_complete(buffer: &str, cursor: usize, cwd: &str) -> Result<()> {
    let request = Request::Complete {
        buffer: buffer.to_string(),
        cursor,
        cwd: cwd.to_string(),
    };
    let response = send_request(&request)?;
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

/// Request completions and print shell-friendly output to stdout.
/// Format: one line per item, tab-separated: display\tinsert\tdescription
/// First line is the item count. Empty output = no completions.
/// Used by shell hooks (no JSON parsing needed in zsh/bash/fish).
pub fn request_complete_shell(buffer: &str, cursor: usize, cwd: &str) -> Result<()> {
    let request = Request::Complete {
        buffer: buffer.to_string(),
        cursor,
        cwd: cwd.to_string(),
    };
    let response = send_request(&request)?;
    match response {
        Response::Completions { items, .. } => {
            println!("{}", items.len());
            for item in &items {
                // Tab-separated: display, insert text, description
                // Replace tabs/newlines in fields to prevent breaking the format
                let sanitize = |s: &str| s.replace(['\t', '\n'], " ");
                let display = sanitize(&item.display);
                let insert = sanitize(&item.insert);
                let desc = sanitize(&item.description);
                println!("{display}\t{insert}\t{desc}");
            }
        }
        _ => {
            // No completions: print nothing (empty output)
        }
    }
    Ok(())
}

/// Notify daemon that user accepted a suggestion.
pub fn request_accept(text: &str) -> Result<()> {
    let request = Request::Accept {
        text: text.to_string(),
    };
    send_request(&request)?;
    Ok(())
}

/// Notify daemon that user dismissed the popup.
pub fn request_dismiss() -> Result<()> {
    send_request(&Request::Dismiss)?;
    Ok(())
}

/// Check daemon status and print it.
pub fn request_status() -> Result<()> {
    match send_request(&Request::Status)? {
        Response::StatusInfo {
            specs_loaded,
            uptime_secs,
            pid,
        } => {
            println!("tabra daemon is running");
            println!("  PID: {pid}");
            println!("  uptime: {uptime_secs}s");
            println!("  specs loaded: {specs_loaded}");
            Ok(())
        }
        Response::Error { message } => {
            bail!("daemon error: {message}");
        }
        _ => {
            bail!("unexpected response from daemon");
        }
    }
}

/// Stop the daemon.
pub fn request_stop() -> Result<()> {
    match send_request(&Request::Stop) {
        Ok(Response::Goodbye) => {
            println!("tabra daemon stopped");
            Ok(())
        }
        Ok(_) => {
            println!("tabra daemon acknowledged stop");
            Ok(())
        }
        Err(e) => {
            // Connection reset is expected when daemon shuts down
            if e.to_string().contains("Connection reset") || e.to_string().contains("Broken pipe") {
                println!("tabra daemon stopped");
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}
