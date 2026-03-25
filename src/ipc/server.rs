//! Unix socket server that runs inside the daemon.
//!
//! Accepts connections, reads a single Request JSON line,
//! dispatches to the engine, writes a Response JSON line, and closes.

use crate::engine::{matcher, parser, resolver};
use crate::ipc::protocol::{CompletionItem, Request, Response};
use crate::render::{overlay, theme::Theme};
use crate::spec::loader::SpecIndex;
use anyhow::{Context as _, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{watch, RwLock};
use tracing::{debug, error, info};

/// Run the IPC server loop.
pub async fn run(
    listener: UnixListener,
    spec_index: Arc<RwLock<SpecIndex>>,
    start_time: Instant,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    info!("IPC server listening");

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _addr)) => {
                        let index = Arc::clone(&spec_index);
                        let start = start_time;
                        let tx = shutdown_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, index, start, tx).await {
                                error!("connection handler error: {e:#}");
                            }
                        });
                    }
                    Err(e) => {
                        error!("accept error: {e}");
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("IPC server shutting down");
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    spec_index: Arc<RwLock<SpecIndex>>,
    start_time: Instant,
    shutdown_tx: watch::Sender<bool>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    tokio::time::timeout(Duration::from_secs(5), buf_reader.read_line(&mut line))
        .await
        .context("IPC read timeout (client connected but sent no data)")??;
    let request = Request::from_json(&line)?;
    debug!("received request: {:?}", request);

    let is_stop = matches!(request, Request::Stop);

    let response = match request {
        Request::Complete {
            buffer,
            cursor,
            cwd,
            terminal_cols,
        } => {
            let index = spec_index.read().await;
            handle_complete(&index, &buffer, cursor, &cwd, terminal_cols)
        }

        Request::Accept { text: _ } => {
            // The shell hook handles the actual insertion.
            // The daemon just acknowledges.
            Response::Ack
        }

        Request::Dismiss => Response::Ack,

        Request::Status => {
            let index = spec_index.read().await;
            Response::StatusInfo {
                specs_loaded: index.len(),
                uptime_secs: start_time.elapsed().as_secs(),
                pid: std::process::id(),
            }
        }

        Request::Stop => Response::Goodbye,
    };

    let response_line = response.to_json_line();
    writer.write_all(response_line.as_bytes()).await?;
    writer.shutdown().await?;

    if is_stop {
        shutdown_tx.send(true).ok();
    }

    Ok(())
}

fn handle_complete(
    index: &SpecIndex,
    buffer: &str,
    cursor: usize,
    cwd: &str,
    terminal_cols: Option<u16>,
) -> Response {
    // Extract the command name (first token)
    let (tokens, _partial) = parser::tokenize(buffer, cursor);
    let cmd_name = match tokens.first() {
        Some(name) => name.as_str(),
        None => return Response::Empty,
    };

    // Look up the spec
    let spec = match index.get(cmd_name) {
        Some(s) => s,
        None => return Response::Empty,
    };

    // Parse the command line
    let ctx = parser::parse(spec, buffer, cursor);

    // Resolve candidate suggestions
    let candidates = resolver::resolve(spec, &ctx, cwd);

    if candidates.is_empty() {
        return Response::Empty;
    }

    // Match and rank
    let scored = matcher::match_suggestions(&ctx.current_token, &candidates, ctx.filter_strategy);

    if scored.is_empty() {
        return Response::Empty;
    }

    let items: Vec<CompletionItem> = scored
        .into_iter()
        .take(50) // max items sent to hook (popup only shows MAX_VISIBLE_ITEMS)
        .map(|s| CompletionItem {
            display: s.suggestion.display_text,
            insert: s.suggestion.insert_text,
            description: s.suggestion.description,
            kind: s.suggestion.kind,
            match_indices: s.match_indices,
            is_dangerous: s.suggestion.is_dangerous,
        })
        .collect();

    // Generate pre-rendered ANSI popup
    let theme = Theme::default();
    let rendered_popup =
        overlay::render_popup(&items, 0, &ctx.current_token, &theme, terminal_cols);

    Response::Completions {
        items,
        selected: 0,
        query: ctx.current_token,
        rendered_popup,
    }
}
