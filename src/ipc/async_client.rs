//! Async IPC client for the session event loop.
//!
//! Uses tokio::net::UnixStream instead of std::os::unix::net::UnixStream.
//! Designed for use inside tokio tasks where blocking I/O is not allowed.

use crate::ipc::protocol::{self, Request, Response};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const WRITE_TIMEOUT: Duration = Duration::from_millis(500);
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Send a request to the daemon and return the response.
pub async fn send_request(request: &Request) -> Result<Response> {
    let socket_path = protocol::socket_path();

    let stream = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(&socket_path))
        .await
        .context("connect timeout")?
        .context("failed to connect to tabra daemon (is it running?)")?;

    let (reader, mut writer) = stream.into_split();

    let request_line = request.to_json_line();
    tokio::time::timeout(WRITE_TIMEOUT, writer.write_all(request_line.as_bytes()))
        .await
        .context("write timeout")?
        .context("write request")?;

    tokio::time::timeout(WRITE_TIMEOUT, writer.flush())
        .await
        .context("flush timeout")?
        .context("flush request")?;

    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    tokio::time::timeout(READ_TIMEOUT, buf_reader.read_line(&mut line))
        .await
        .context("read timeout")?
        .context("read response")?;

    Response::from_json(&line).context("parse response")
}

/// Request completions from the daemon.
pub async fn complete(
    buffer: &str,
    cursor: usize,
    cwd: &str,
    cols: Option<u16>,
) -> Result<Response> {
    let request = Request::Complete {
        buffer: buffer.to_string(),
        cursor,
        cwd: cwd.to_string(),
        terminal_cols: cols,
    };
    send_request(&request).await
}

/// Check daemon status.
pub async fn status() -> Result<Response> {
    send_request(&Request::Status).await
}
