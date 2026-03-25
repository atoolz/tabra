//! Integration tests for the Tabra daemon.
//!
//! These tests spawn a real daemon, send completion requests, and verify
//! the responses. They require compiled specs in the `specs/` directory.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Helper to manage a daemon process for testing.
struct TestDaemon {
    process: Child,
    socket_path: PathBuf,
    specs_dir: PathBuf,
}

impl TestDaemon {
    /// Spawn a daemon with a temporary specs directory.
    fn spawn(specs: &[&str]) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!("tabra-test-{}-{}", std::process::id(), id));
        let specs_dir = tmp.join("specs");
        let socket_path = tmp.join("tabra.sock");

        // Create temp dirs
        std::fs::create_dir_all(&specs_dir).unwrap();

        // Copy requested specs from the project's specs/ directory
        let project_specs = Path::new(env!("CARGO_MANIFEST_DIR")).join("specs");
        for spec_name in specs {
            let src = project_specs.join(format!("{spec_name}.json"));
            if src.exists() {
                let dst = specs_dir.join(format!("{spec_name}.json"));
                std::fs::copy(&src, &dst).unwrap();
            }
        }

        // Set XDG_RUNTIME_DIR so daemon uses our socket path
        let binary = env!("CARGO_BIN_EXE_tabra");

        let process = Command::new(binary)
            .arg("daemon")
            .arg("--specs-dir")
            .arg(&specs_dir)
            .env("XDG_RUNTIME_DIR", &tmp)
            .env("RUST_LOG", "tabra=warn")
            .spawn()
            .expect("failed to spawn tabra daemon");

        let daemon = Self {
            process,
            socket_path,
            specs_dir,
        };

        // Wait for socket to appear
        for _ in 0..50 {
            if daemon.socket_path.exists() {
                return daemon;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!(
            "daemon did not create socket at {:?} within 5s",
            daemon.socket_path
        );
    }

    /// Send a JSON request and get a JSON response.
    fn request(&self, json: &str) -> String {
        let stream = UnixStream::connect(&self.socket_path).expect("failed to connect to daemon");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let mut stream = stream;
        let mut request = json.to_string();
        if !request.ends_with('\n') {
            request.push('\n');
        }
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        line
    }

    /// Send a completion request and return the parsed response.
    fn complete(&self, buffer: &str, cursor: usize, cwd: &str) -> serde_json::Value {
        let req = serde_json::json!({
            "type": "complete",
            "buffer": buffer,
            "cursor": cursor,
            "cwd": cwd,
        });
        let resp = self.request(&req.to_string());
        serde_json::from_str(&resp).expect("failed to parse response")
    }

    /// Send a status request and return the response.
    fn status(&self) -> serde_json::Value {
        let req = serde_json::json!({"type": "status"});
        let resp = self.request(&req.to_string());
        serde_json::from_str(&resp).expect("failed to parse status response")
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        // Send stop request (best-effort)
        let _ = self.request(r#"{"type":"stop"}"#);
        // Wait briefly then kill
        std::thread::sleep(Duration::from_millis(200));
        let _ = self.process.kill();
        let _ = self.process.wait();
        // Cleanup temp dir
        let _ = std::fs::remove_dir_all(self.specs_dir.parent().unwrap());
    }
}

#[test]
fn test_daemon_status() {
    let daemon = TestDaemon::spawn(&["git"]);
    let status = daemon.status();

    assert_eq!(status["type"], "status_info");
    assert!(status["specs_loaded"].as_u64().unwrap() >= 1);
    assert!(status["pid"].as_u64().unwrap() > 0);
}

#[test]
fn test_complete_git_subcommands() {
    let daemon = TestDaemon::spawn(&["git"]);
    let resp = daemon.complete("git ", 4, "/tmp");

    assert_eq!(resp["type"], "completions");
    let items = resp["items"].as_array().expect("items should be an array");
    assert!(!items.is_empty(), "git should have completions");

    // Check that common subcommands are present
    let displays: Vec<&str> = items
        .iter()
        .filter_map(|item| item["display"].as_str())
        .collect();

    assert!(
        displays.iter().any(|d| d.contains("commit")),
        "should include 'commit', got: {:?}",
        &displays[..displays.len().min(10)]
    );
    assert!(
        displays.iter().any(|d| d.contains("checkout")),
        "should include 'checkout'"
    );
    assert!(
        displays.iter().any(|d| d.contains("push")),
        "should include 'push'"
    );
}

#[test]
fn test_complete_git_partial_match() {
    let daemon = TestDaemon::spawn(&["git"]);
    let resp = daemon.complete("git ch", 6, "/tmp");

    assert_eq!(resp["type"], "completions");
    let items = resp["items"].as_array().expect("items should be an array");
    assert!(!items.is_empty(), "git ch should have matches");

    let displays: Vec<&str> = items
        .iter()
        .filter_map(|item| item["display"].as_str())
        .collect();

    // "ch" should fuzzy-match checkout, cherry, cherry-pick, check-*
    assert!(
        displays.iter().any(|d| d.contains("checkout")),
        "should match 'checkout' for query 'ch', got: {:?}",
        displays
    );
}

#[test]
fn test_complete_unknown_command() {
    let daemon = TestDaemon::spawn(&["git"]);
    let resp = daemon.complete("nonexistent ", 12, "/tmp");

    assert_eq!(resp["type"], "empty", "unknown command should return empty");
}

#[test]
fn test_complete_git_options() {
    let daemon = TestDaemon::spawn(&["git"]);
    // Use "--" as partial token to trigger option matching
    let resp = daemon.complete("git commit --", 13, "/tmp");

    assert_eq!(resp["type"], "completions");
    let items = resp["items"].as_array().expect("items should be an array");

    let displays: Vec<&str> = items
        .iter()
        .filter_map(|item| item["display"].as_str())
        .collect();

    // Should show options starting with --
    assert!(
        displays.iter().any(|d| d.starts_with("--")),
        "should have long options for git commit, got: {:?}",
        &displays[..displays.len().min(10)]
    );
}

#[test]
fn test_complete_docker_subcommands() {
    let daemon = TestDaemon::spawn(&["docker"]);
    let resp = daemon.complete("docker ", 7, "/tmp");

    assert_eq!(resp["type"], "completions");
    let items = resp["items"].as_array().expect("items should be an array");
    assert!(!items.is_empty(), "docker should have completions");

    let displays: Vec<&str> = items
        .iter()
        .filter_map(|item| item["display"].as_str())
        .collect();

    assert!(
        displays.iter().any(|d| d.contains("run")),
        "should include 'run'"
    );
    assert!(
        displays.iter().any(|d| d.contains("build")),
        "should include 'build'"
    );
}

#[test]
fn test_multiple_specs_loaded() {
    let daemon = TestDaemon::spawn(&["git", "docker", "kubectl"]);
    let status = daemon.status();

    // All 3 specs should be present in specs/ (git, docker, kubectl)
    let loaded = status["specs_loaded"].as_u64().unwrap();
    assert!(
        loaded >= 3,
        "should load at least 3 specs, got {loaded}. Ensure specs/git.json, specs/docker.json, specs/kubectl.json exist."
    );

    // Each command should work
    let git_resp = daemon.complete("git ", 4, "/tmp");
    assert_eq!(git_resp["type"], "completions");

    let docker_resp = daemon.complete("docker ", 7, "/tmp");
    assert_eq!(docker_resp["type"], "completions");

    let kubectl_resp = daemon.complete("kubectl ", 8, "/tmp");
    assert_eq!(kubectl_resp["type"], "completions");
}
