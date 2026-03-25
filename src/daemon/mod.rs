//! Daemon process: loads specs, listens on Unix socket, serves completions.
//!
//! The daemon is a long-running background process that:
//! 1. Loads all spec JSON files from the specs directory into memory
//! 2. Watches the specs directory for changes (hot-reload)
//! 3. Listens on a Unix domain socket for IPC requests
//! 4. Handles completion requests by parsing, resolving, and matching
//! 5. Responds with ranked completion items

use crate::ipc::{protocol, server};
use crate::spec::loader::{self, SpecIndex};
use anyhow::{Context, Result};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UnixListener;
use tokio::sync::{watch, RwLock};
use tracing::{error, info, warn};

/// Run the tabra daemon.
pub fn run(specs_dir: Option<PathBuf>) -> Result<()> {
    let specs_dir = specs_dir.unwrap_or_else(loader::default_specs_dir);

    // Ensure specs directory exists
    std::fs::create_dir_all(&specs_dir)
        .with_context(|| format!("creating specs dir: {:?}", specs_dir))?;

    let rt = tokio::runtime::Runtime::new().context("creating tokio runtime")?;
    rt.block_on(async_run(specs_dir))
}

async fn async_run(specs_dir: PathBuf) -> Result<()> {
    let start_time = Instant::now();

    // Load specs
    let spec_index = SpecIndex::load(specs_dir.clone()).context("loading specs")?;
    info!("loaded {} specs", spec_index.len());
    let spec_index = Arc::new(RwLock::new(spec_index));

    // Setup socket
    let socket_path = protocol::socket_path();
    // Ensure parent directory exists
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // Try to bind; if socket exists, check if a daemon is already running
    let listener = match UnixListener::bind(&socket_path) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Check if existing socket is live by trying to connect
            if std::os::unix::net::UnixStream::connect(&socket_path).is_ok() {
                anyhow::bail!(
                    "another tabra daemon is already running on {:?}",
                    socket_path
                );
            }
            // Socket is stale, remove and retry
            std::fs::remove_file(&socket_path).ok();
            UnixListener::bind(&socket_path)
                .with_context(|| format!("binding {:?} after removing stale socket", socket_path))?
        }
        Err(e) => return Err(e).with_context(|| format!("binding {:?}", socket_path)),
    };
    // Restrict socket to owner only (prevent other local users from connecting)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("setting socket permissions on {:?}", socket_path))?;
    }
    info!("listening on {:?}", socket_path);

    // Write PID file
    let pid_path = socket_path.with_extension("pid");
    std::fs::write(&pid_path, std::process::id().to_string()).ok();

    // Shutdown channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // File watcher for spec hot-reload
    // Capture the tokio runtime handle so we can spawn from notify's OS thread
    let watcher_handle = tokio::runtime::Handle::current();
    let watcher_index = Arc::clone(&spec_index);
    let mut watcher =
        notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
            Ok(event) => {
                let idx = watcher_index.clone();
                let handle = watcher_handle.clone();
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        for path in &event.paths {
                            let idx = idx.clone();
                            let path = path.clone();
                            handle.spawn(async move {
                                let mut index = idx.write().await;
                                if let Err(e) = index.reload_file(&path) {
                                    warn!("hot-reload failed for {:?}: {e:#}", path);
                                }
                            });
                        }
                    }
                    EventKind::Remove(_) => {
                        for path in &event.paths {
                            let idx = idx.clone();
                            let path = path.clone();
                            handle.spawn(async move {
                                let mut index = idx.write().await;
                                index.remove_file(&path);
                            });
                        }
                    }
                    _ => {}
                }
            }
            Err(e) => {
                error!("file watcher error: {e}");
            }
        })
        .context("creating file watcher")?;

    watcher
        .watch(&specs_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("watching {:?}", specs_dir))?;
    info!("watching specs dir for changes: {:?}", specs_dir);

    // Handle signals
    {
        let tx = shutdown_tx.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("received SIGINT, shutting down");
            tx.send(true).ok();
        });
    }

    // Run IPC server
    server::run(listener, spec_index, start_time, shutdown_tx, shutdown_rx).await?;

    // Cleanup
    std::fs::remove_file(&socket_path).ok();
    std::fs::remove_file(&pid_path).ok();
    info!("daemon stopped");

    Ok(())
}
