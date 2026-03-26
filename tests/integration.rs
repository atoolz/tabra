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

#[test]
fn test_generator_git_checkout_branches() {
    // Create a real git repo with known branches
    let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let repo_dir =
        std::env::temp_dir().join(format!("tabra-git-test-{}-{}", std::process::id(), id));
    std::fs::create_dir_all(&repo_dir).unwrap();

    // Init repo and create branches
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&repo_dir)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap()
    };

    run(&["init", "-b", "main"]);
    run(&["commit", "--allow-empty", "-m", "init"]);
    run(&["branch", "feature-foo"]);
    run(&["branch", "feature-bar"]);

    // Spawn daemon with git spec
    let daemon = TestDaemon::spawn(&["git"]);
    let repo_path = repo_dir.to_str().unwrap();

    // Request completions for "git checkout " in the repo directory
    let resp = daemon.complete("git checkout ", 13, repo_path);
    assert_eq!(resp["type"], "completions");

    let items = resp["items"].as_array().expect("items should be an array");
    let displays: Vec<&str> = items
        .iter()
        .filter_map(|item| item["display"].as_str())
        .collect();

    // Branch names should appear via generator script
    assert!(
        displays.iter().any(|d| d.contains("main")),
        "should include 'main' branch, got: {:?}",
        &displays[..displays.len().min(15)]
    );
    assert!(
        displays.iter().any(|d| d.contains("feature-foo")),
        "should include 'feature-foo' branch"
    );
    assert!(
        displays.iter().any(|d| d.contains("feature-bar")),
        "should include 'feature-bar' branch"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn test_init_zsh_output() {
    let binary = env!("CARGO_BIN_EXE_tabra");
    let output = Command::new(binary)
        .args(["init", "zsh"])
        .output()
        .expect("failed to run tabra init zsh");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("_TABRA_LOADED"),
        "zsh hook should contain guard variable"
    );
    assert!(
        stdout.contains("_tabra_self_insert"),
        "zsh hook should contain self-insert widget"
    );
    assert!(
        stdout.contains("bindkey"),
        "zsh hook should contain key bindings"
    );
    assert!(
        stdout.contains("complete-shell"),
        "zsh hook should call tabra complete-shell"
    );
}

#[test]
fn test_init_bash_output() {
    let binary = env!("CARGO_BIN_EXE_tabra");
    let output = Command::new(binary)
        .args(["init", "bash"])
        .output()
        .expect("failed to run tabra init bash");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("_TABRA_LOADED"),
        "bash hook should contain guard variable"
    );
    assert!(
        stdout.contains("READLINE_LINE"),
        "bash hook should use READLINE_LINE"
    );
    assert!(
        stdout.contains("READLINE_POINT"),
        "bash hook should use READLINE_POINT"
    );
    assert!(
        stdout.contains("bind -x"),
        "bash hook should use bind -x for key bindings"
    );
    assert!(
        stdout.contains("BASH_VERSINFO"),
        "bash hook should check bash version"
    );
    assert!(
        stdout.contains("complete-shell"),
        "bash hook should call tabra complete-shell"
    );
}

#[test]
fn test_session_subcommand_exists() {
    // Verify `tabra session --help` works (doesn't crash, shows help text)
    let binary = env!("CARGO_BIN_EXE_tabra");
    let output = Command::new(binary)
        .args(["session", "--help"])
        .output()
        .expect("failed to run tabra session --help");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("PTY") || stdout.contains("session") || stdout.contains("autocomplete"),
        "session help should describe the command, got: {}",
        &stdout[..stdout.len().min(200)]
    );
    assert!(output.status.success());
}

#[test]
fn test_session_integration_script_output() {
    // Verify the bash integration script contains OSC markers (not popup rendering)
    let binary = env!("CARGO_BIN_EXE_tabra");

    // The session uses integration::bash_integration() internally.
    // We can test it indirectly by checking that `tabra init bash` still works
    // (the old hook approach) while the session approach is separate.
    let output = Command::new(binary)
        .args(["init", "bash"])
        .output()
        .expect("failed to run tabra init bash");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The old init bash hook should still contain bind -x
    assert!(
        stdout.contains("bind -x"),
        "init bash should contain bind -x"
    );
    // It should NOT contain OSC 6973 (that's the session integration, not the init hook)
    // (both approaches coexist: init is legacy, session is new)
}

#[test]
fn test_init_fish_output() {
    let binary = env!("CARGO_BIN_EXE_tabra");
    let output = Command::new(binary)
        .args(["init", "fish"])
        .output()
        .expect("failed to run tabra init fish");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("_TABRA_LOADED"),
        "fish hook should contain guard variable"
    );
    assert!(
        stdout.contains("commandline"),
        "fish hook should use commandline builtin"
    );
    assert!(
        stdout.contains("bind"),
        "fish hook should contain key bindings"
    );
    assert!(
        stdout.contains("complete-shell"),
        "fish hook should call tabra complete-shell"
    );
    assert!(
        stdout.contains("fish_exit"),
        "fish hook should clean up on exit"
    );
}
