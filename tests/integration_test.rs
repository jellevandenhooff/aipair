//! Integration tests for aipair
//!
//! These tests spin up a real server against a temporary jj repository
//! and verify the full flow works end-to-end.

use reqwest::Client;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;
use tempfile::TempDir;

struct TestHarness {
    _temp_dir: TempDir,
    server: Child,
    client: Client,
    base_url: String,
}

impl TestHarness {
    async fn new() -> Self {
        // Create a temporary directory with a jj repo
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let repo_path = temp_dir.path();

        // Initialize git repo (jj needs this)
        Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Failed to init git repo");

        // Initialize jj repo
        Command::new("jj")
            .args(["git", "init", "--colocate"])
            .current_dir(repo_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Failed to init jj repo");

        // Create a test file and commit
        std::fs::write(repo_path.join("test.txt"), "hello world\n").unwrap();

        Command::new("jj")
            .args(["describe", "-m", "Initial commit"])
            .current_dir(repo_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Failed to describe");

        // Create main bookmark
        Command::new("jj")
            .args(["bookmark", "create", "main", "-r", "@"])
            .current_dir(repo_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Failed to create main bookmark");

        // Create a new change with modifications
        Command::new("jj")
            .args(["new", "-m", "Add more content"])
            .current_dir(repo_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Failed to create new change");

        std::fs::write(repo_path.join("test.txt"), "hello world\nmore content\n").unwrap();

        // Find a free port
        let port = portpicker::pick_unused_port().expect("No free port");
        let base_url = format!("http://localhost:{}", port);

        // Get the path to the built binary
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let binary_path = format!("{}/target/debug/aipair", manifest_dir);

        // Start the server
        let server = Command::new(&binary_path)
            .args(["serve", "--port", &port.to_string()])
            .current_dir(repo_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to start server");

        // Wait for server to be ready
        let client = Client::new();
        for _ in 0..50 {
            if client.get(&format!("{}/api/health", base_url)).send().await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Self {
            _temp_dir: temp_dir,
            server,
            client,
            base_url,
        }
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(&format!("{}{}", self.base_url, path))
            .send()
            .await
            .expect("Request failed")
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        self.client
            .post(&format!("{}{}", self.base_url, path))
            .json(&body)
            .send()
            .await
            .expect("Request failed")
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        let _ = self.server.kill();
    }
}

#[tokio::test]
async fn test_health_endpoint() {
    let harness = TestHarness::new().await;
    let response = harness.get("/api/health").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn test_list_changes() {
    let harness = TestHarness::new().await;
    let response = harness.get("/api/changes").await;
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    let changes = body["changes"].as_array().unwrap();
    assert!(!changes.is_empty());
}

#[tokio::test]
async fn test_get_diff() {
    let harness = TestHarness::new().await;

    // First get the changes to find a change_id
    let response = harness.get("/api/changes").await;
    let body: serde_json::Value = response.json().await.unwrap();
    let changes = body["changes"].as_array().unwrap();
    let change_id = changes[0]["change_id"].as_str().unwrap();

    // Now get the diff
    let response = harness.get(&format!("/api/changes/{}/diff", change_id)).await;
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["diff"]["raw"].as_str().is_some());
}

#[tokio::test]
async fn test_review_workflow() {
    let harness = TestHarness::new().await;

    // Get a change_id
    let response = harness.get("/api/changes").await;
    let body: serde_json::Value = response.json().await.unwrap();
    let changes = body["changes"].as_array().unwrap();
    let change_id = changes[0]["change_id"].as_str().unwrap();

    // Create a review
    let response = harness
        .post(
            &format!("/api/changes/{}/review", change_id),
            serde_json::json!({ "base": "@-" }),
        )
        .await;
    assert_eq!(response.status(), 200);

    // Add a comment
    let response = harness
        .post(
            &format!("/api/changes/{}/comments", change_id),
            serde_json::json!({
                "file": "test.txt",
                "line_start": 1,
                "line_end": 2,
                "text": "This looks good!"
            }),
        )
        .await;
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["review"]["threads"].as_array().unwrap().len(), 1);
    assert!(!body["thread_id"].as_str().unwrap().is_empty());

    // Verify the review has the comment
    let response = harness.get(&format!("/api/changes/{}/review", change_id)).await;
    let body: serde_json::Value = response.json().await.unwrap();
    let threads = body["review"]["threads"].as_array().unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0]["comments"][0]["text"], "This looks good!");
}

#[tokio::test]
async fn test_thread_relocation_after_edit() {
    // Custom setup: file with multiple lines so we can track line movement
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path();

    Command::new("jj")
        .args(["git", "init"])
        .current_dir(repo_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("Failed to init jj repo");

    // Create a file with 10 lines
    let initial_content: String = (1..=10).map(|i| format!("line {}\n", i)).collect();
    std::fs::write(repo_path.join("code.rs"), &initial_content).unwrap();

    Command::new("jj")
        .args(["describe", "-m", "Initial commit"])
        .current_dir(repo_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();

    // Create working change and move main to it
    Command::new("jj")
        .args(["new", "-m", "Working change"])
        .current_dir(repo_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();

    Command::new("jj")
        .args(["bookmark", "create", "main", "-r", "@"])
        .current_dir(repo_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();

    // Start server
    let port = portpicker::pick_unused_port().expect("No free port");
    let base_url = format!("http://localhost:{}", port);
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let binary_path = format!("{}/target/debug/aipair", manifest_dir);

    let mut server = Command::new(&binary_path)
        .args(["serve", "--port", &port.to_string()])
        .current_dir(repo_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    let client = Client::new();
    for _ in 0..50 {
        if client
            .get(&format!("{}/api/health", base_url))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Get change_id for the working change
    let response = client
        .get(&format!("{}/api/changes", base_url))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let changes = body["changes"].as_array().unwrap();
    let change_id = changes[0]["change_id"].as_str().unwrap().to_string();

    // Create a review
    client
        .post(&format!("{}/api/changes/{}/review", base_url, change_id))
        .json(&serde_json::json!({ "base": "@-" }))
        .send()
        .await
        .unwrap();

    // Add a comment on line 5
    let response = client
        .post(&format!("{}/api/changes/{}/comments", base_url, change_id))
        .json(&serde_json::json!({
            "file": "code.rs",
            "line_start": 5,
            "line_end": 5,
            "text": "Check this logic"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // Verify initial display positions match stored positions
    let response = client
        .get(&format!("{}/api/changes/{}/review", base_url, change_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let thread = &body["review"]["threads"][0];
    assert_eq!(thread["line_start"], 5);
    assert_eq!(thread["line_end"], 5);
    assert_eq!(thread["display_line_start"], 5);
    assert_eq!(thread["display_line_end"], 5);
    assert_eq!(thread["is_displaced"], false);
    assert_eq!(thread["is_deleted"], false);

    // Now edit the file: insert 3 lines at the top (pushing line 5 → line 8)
    let mut new_content = "new line A\nnew line B\nnew line C\n".to_string();
    new_content.push_str(&initial_content);
    std::fs::write(repo_path.join("code.rs"), &new_content).unwrap();

    // Fetch review again — thread should now have updated display positions
    let response = client
        .get(&format!("{}/api/changes/{}/review", base_url, change_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let thread = &body["review"]["threads"][0];

    // Stored positions should be unchanged
    assert_eq!(thread["line_start"], 5);
    assert_eq!(thread["line_end"], 5);
    // Display positions should reflect the shift
    assert_eq!(thread["display_line_start"], 8, "expected line to shift from 5 to 8 after inserting 3 lines above");
    assert_eq!(thread["display_line_end"], 8);
    assert_eq!(thread["is_displaced"], true);
    assert_eq!(thread["is_deleted"], false);

    // Now delete line 8 (was originally line 5) to test deletion
    let lines: Vec<&str> = new_content.lines().collect();
    let deleted_content: String = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 7) // 0-indexed line 7 = line 8
        .map(|(_, l)| format!("{}\n", l))
        .collect();
    std::fs::write(repo_path.join("code.rs"), &deleted_content).unwrap();

    let response = client
        .get(&format!("{}/api/changes/{}/review", base_url, change_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let thread = &body["review"]["threads"][0];
    assert_eq!(thread["is_deleted"], true, "thread should be marked deleted after its line was removed");

    let _ = server.kill();
}

// --- Session lifecycle helpers (no server needed) ---

fn aipair_binary() -> String {
    format!("{}/target/debug/aipair", env!("CARGO_MANIFEST_DIR"))
}

fn aipair(dir: &Path, args: &[&str]) -> Output {
    Command::new(aipair_binary())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Failed to run aipair")
}

fn aipair_ok(dir: &Path, args: &[&str]) -> String {
    let output = aipair(dir, args);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "aipair {:?} failed:\nstdout: {}\nstderr: {}",
        args, stdout, stderr
    );
    format!("{}{}", stdout, stderr)
}

fn jj_cmd(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("jj")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Failed to run jj");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "jj {:?} failed:\nstdout: {}\nstderr: {}",
        args, stdout, stderr
    );
    format!("{}{}", stdout, stderr)
}

/// Start a server in a directory, returning the child process and base URL
async fn start_server(dir: &Path) -> (Child, String) {
    let port = portpicker::pick_unused_port().expect("No free port");
    let base_url = format!("http://localhost:{}", port);

    let server = Command::new(aipair_binary())
        .args(["serve", "--port", &port.to_string()])
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    // Wait for server to be ready
    let client = Client::new();
    for _ in 0..50 {
        if client.get(&format!("{}/api/health", base_url)).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    (server, base_url)
}

#[test]
fn test_session_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let main_dir = temp_dir.path().join("main");
    std::fs::create_dir(&main_dir).unwrap();

    // 1. Setup temp jj repo
    jj_cmd(&main_dir, &["git", "init", "--colocate"]);
    std::fs::write(main_dir.join(".gitignore"), ".aipair/\n").unwrap();
    std::fs::write(main_dir.join("test.txt"), "hello\n").unwrap();
    jj_cmd(&main_dir, &["describe", "-m", "Initial commit"]);
    jj_cmd(&main_dir, &["bookmark", "create", "main", "-r", "@"]);
    jj_cmd(&main_dir, &["new", "-m", "wc"]);

    // 2. session new — creates clone + marker
    let out = aipair_ok(&main_dir, &["session", "new", "test-session"]);
    assert!(
        out.contains("Session 'test-session' created!"),
        "Expected creation message, got: {}",
        out
    );
    let clone_dir = main_dir.join(".aipair/sessions/test-session/repo");
    assert!(clone_dir.exists(), "Clone directory should exist");
    assert!(
        clone_dir.join(".aipair/session.json").exists(),
        "Marker file should exist"
    );

    // 3. session list from main
    let out = aipair_ok(&main_dir, &["session", "list"]);
    assert!(out.contains("test-session"), "list: {}", out);
    assert!(out.contains("active"), "list status: {}", out);

    // 4. Make a change in clone
    std::fs::write(clone_dir.join("session-file.txt"), "from session\n").unwrap();
    jj_cmd(&clone_dir, &["describe", "-m", "Session work"]);

    // 5. push from clone
    let out = aipair_ok(&clone_dir, &["push", "-m", "First push", "--rev", "@"]);
    assert!(out.contains("Pushed!"), "push: {}", out);

    // 6. session list from main — should show push summary
    let out = aipair_ok(&main_dir, &["session", "list"]);
    assert!(out.contains("First push"), "list after push: {}", out);

    // 6b. Create a second session and push work into it (simulates parallel sessions)
    aipair_ok(&main_dir, &["session", "new", "other-session"]);
    let other_clone_dir = main_dir.join(".aipair/sessions/other-session/repo");
    std::fs::write(other_clone_dir.join("other-session-file.txt"), "other work\n").unwrap();
    jj_cmd(&other_clone_dir, &["describe", "-m", "Other session work"]);
    aipair_ok(&other_clone_dir, &["push", "-m", "Other push", "--rev", "@"]);

    // 6c. Verify scoped clone: test-session's clone should NOT see other-session's commits
    let visible_str = jj_cmd(&clone_dir, &["log", "--no-graph", "-r", "visible_heads()", "-T", r#"bookmarks ++ "\n""#]);
    assert!(
        !visible_str.contains("other-session"),
        "test-session's clone should not see other-session's bookmarks. Got: {}",
        visible_str
    );

    // 7. status from clone
    let out = aipair_ok(&clone_dir, &["status"]);
    assert!(out.contains("test-session"), "status: {}", out);

    // 8. Advance main in main repo (simulates other work landing)
    jj_cmd(&main_dir, &["new", "main", "-m", "Other work"]);
    std::fs::write(main_dir.join("other.txt"), "other content\n").unwrap();
    jj_cmd(&main_dir, &["bookmark", "set", "main", "-r", "@"]);

    // 9. pull from clone — rebase onto updated main
    let out = aipair_ok(&clone_dir, &["pull"]);
    assert!(out.contains("no conflicts"), "pull: {}", out);

    // 10. push after rebase (@ is the empty WC commit after rebase, @- is the work)
    let out = aipair_ok(&clone_dir, &["push", "-m", "After rebase", "--rev", "@-"]);
    assert!(out.contains("Pushed!"), "push after rebase: {}", out);

    // 11. session merge from main
    let out = aipair_ok(&main_dir, &["session", "merge", "test-session"]);
    assert!(out.contains("merged"), "merge: {}", out);

    // 12. session list — should show merged status
    let out = aipair_ok(&main_dir, &["session", "list"]);
    assert!(out.contains("merged"), "list after merge: {}", out);

    // 13. status from main — test-session merged, other-session still active
    let out = aipair_ok(&main_dir, &["status"]);
    assert!(
        !out.contains("test-session"),
        "test-session should not be listed as active after merge: {}",
        out
    );
    assert!(
        out.contains("other-session"),
        "other-session should still be active: {}",
        out
    );
}

#[tokio::test]
async fn test_session_push_api() {
    let temp_dir = TempDir::new().unwrap();
    let main_dir = temp_dir.path().join("main");
    std::fs::create_dir(&main_dir).unwrap();

    // Setup main repo
    jj_cmd(&main_dir, &["git", "init", "--colocate"]);
    std::fs::write(main_dir.join(".gitignore"), ".aipair/\n").unwrap();
    std::fs::write(main_dir.join("test.txt"), "hello\n").unwrap();
    jj_cmd(&main_dir, &["describe", "-m", "Initial commit"]);
    jj_cmd(&main_dir, &["bookmark", "create", "main", "-r", "@"]);
    jj_cmd(&main_dir, &["new", "-m", "wc"]);

    // Create session, make a change, push
    aipair_ok(&main_dir, &["session", "new", "test-session"]);
    let clone_dir = main_dir.join(".aipair/sessions/test-session/repo");
    std::fs::write(clone_dir.join("feature.txt"), "new feature\n").unwrap();
    jj_cmd(&clone_dir, &["describe", "-m", "Add feature"]);
    aipair_ok(&clone_dir, &["push", "-m", "Feature push", "--rev", "@"]);

    // Start server and query session changes API
    let (mut server, base_url) = start_server(&main_dir).await;
    let client = Client::new();

    // Live view should show the pushed change
    let resp = client
        .get(&format!("{}/api/sessions/test-session/changes", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let changes = body["changes"].as_array().unwrap();
    assert!(!changes.is_empty(), "Live view should show pushed changes");
    let change_id = changes.iter()
        .find(|c| c["description"].as_str().unwrap_or("").contains("Add feature"))
        .expect("Should find the 'Add feature' change in live view");
    assert!(!change_id["change_id"].as_str().unwrap().is_empty());

    // Historical view (version=0) should also work
    let resp = client
        .get(&format!("{}/api/sessions/test-session/changes?version=0", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "version=0 should succeed");
    let body: serde_json::Value = resp.json().await.unwrap();
    let changes = body["changes"].as_array().unwrap();
    assert!(!changes.is_empty(), "version=0 should have changes");

    let _ = server.kill();
    let _ = server.wait();
}

#[tokio::test]
async fn test_session_feedback_respond() {
    let temp_dir = TempDir::new().unwrap();
    let main_dir = temp_dir.path().join("main");
    std::fs::create_dir(&main_dir).unwrap();

    // Setup main repo
    jj_cmd(&main_dir, &["git", "init", "--colocate"]);
    std::fs::write(main_dir.join(".gitignore"), ".aipair/\n").unwrap();
    std::fs::write(main_dir.join("test.txt"), "hello\nworld\n").unwrap();
    jj_cmd(&main_dir, &["describe", "-m", "Initial commit"]);
    jj_cmd(&main_dir, &["bookmark", "create", "main", "-r", "@"]);
    jj_cmd(&main_dir, &["new", "-m", "wc"]);

    // Create session + push
    aipair_ok(&main_dir, &["session", "new", "test-session"]);
    let clone_dir = main_dir.join(".aipair/sessions/test-session/repo");
    std::fs::write(clone_dir.join("feature.txt"), "new feature\n").unwrap();
    jj_cmd(&clone_dir, &["describe", "-m", "Add feature"]);
    aipair_ok(&clone_dir, &["push", "-m", "Feature push", "--rev", "@"]);

    // Start server in main repo to create reviews via API
    let (mut server, base_url) = start_server(&main_dir).await;
    let client = Client::new();

    // Get session changes to find the session change_id
    let resp = client
        .get(&format!("{}/api/sessions/test-session/changes", base_url))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let changes = body["changes"].as_array().unwrap();
    assert!(!changes.is_empty(), "Should have session changes");
    let change_id = changes[0]["change_id"].as_str().unwrap().to_string();

    // Create a review and add a comment via API
    client
        .post(&format!("{}/api/changes/{}/review", base_url, change_id))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(&format!("{}/api/changes/{}/comments", base_url, change_id))
        .json(&serde_json::json!({
            "file": "feature.txt",
            "line_start": 1,
            "line_end": 1,
            "text": "Please add tests for this"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let comment_body: serde_json::Value = resp.json().await.unwrap();
    let thread_id = comment_body["thread_id"].as_str().unwrap().to_string();

    // Kill server before running CLI commands (to avoid locking)
    let _ = server.kill();
    let _ = server.wait();

    // `aipair feedback` from clone → should show the comment
    let out = aipair_ok(&clone_dir, &["feedback"]);
    assert!(
        out.contains("Please add tests for this"),
        "feedback should contain comment text: {}",
        out
    );

    // `aipair respond` from clone with --resolve
    let out = aipair_ok(
        &clone_dir,
        &[
            "respond",
            &change_id[..8],
            &thread_id[..8],
            "Added tests",
            "--resolve",
        ],
    );
    assert!(
        out.contains("Responded") && out.contains("resolved"),
        "respond output: {}",
        out
    );

    // `aipair feedback` again → should show no pending feedback
    let out = aipair_ok(&clone_dir, &["feedback"]);
    assert!(
        out.contains("No pending feedback"),
        "feedback after resolve: {}",
        out
    );
}

#[tokio::test]
async fn test_session_api() {
    let temp_dir = TempDir::new().unwrap();
    let main_dir = temp_dir.path().join("main");
    std::fs::create_dir(&main_dir).unwrap();

    // Setup main repo
    jj_cmd(&main_dir, &["git", "init", "--colocate"]);
    std::fs::write(main_dir.join(".gitignore"), ".aipair/\n").unwrap();
    std::fs::write(main_dir.join("test.txt"), "hello\n").unwrap();
    jj_cmd(&main_dir, &["describe", "-m", "Initial commit"]);
    jj_cmd(&main_dir, &["bookmark", "create", "main", "-r", "@"]);
    jj_cmd(&main_dir, &["new", "-m", "wc"]);

    // Create session + push
    aipair_ok(&main_dir, &["session", "new", "api-session"]);
    let clone_dir = main_dir.join(".aipair/sessions/api-session/repo");
    std::fs::write(clone_dir.join("api-file.txt"), "api feature\n").unwrap();
    jj_cmd(&clone_dir, &["describe", "-m", "API feature"]);
    aipair_ok(&clone_dir, &["push", "-m", "API push", "--rev", "@"]);

    // Start server
    let (mut server, base_url) = start_server(&main_dir).await;
    let client = Client::new();

    // GET /api/changes → verify sessions metadata
    let resp = client
        .get(&format!("{}/api/changes", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    // Check sessions array in response
    let sessions = body["sessions"].as_array().unwrap();
    assert!(!sessions.is_empty(), "Should have sessions in response");
    let session = sessions
        .iter()
        .find(|s| s["name"].as_str() == Some("api-session"))
        .expect("Should find api-session");
    assert_eq!(session["status"], "active");
    assert_eq!(session["push_count"], 1);
    assert_eq!(session["base_bookmark"], "main");
    assert!(session["change_count"].as_u64().unwrap() > 0, "Should have change_count");

    // GET /api/sessions/api-session/changes → verify session-scoped changes
    let resp = client
        .get(&format!("{}/api/sessions/api-session/changes", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_changes = body["changes"].as_array().unwrap();
    assert!(
        !session_changes.is_empty(),
        "Should have session-scoped changes. Response: {:?}",
        body
    );
    assert_eq!(
        session_changes[0]["session_name"].as_str(),
        Some("api-session"),
        "Session changes should have session_name"
    );

    // GET /api/sessions/api-session/changes?version=live → verify live endpoint
    let resp = client
        .get(&format!("{}/api/sessions/api-session/changes?version=live", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["changes"].as_array().is_some(), "Live should have changes");

    // POST /api/sessions/api-session/merge
    let resp = client
        .post(&format!("{}/api/sessions/api-session/merge", base_url))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true, "Merge should succeed: {:?}", body);

    // GET /api/changes → session should now be merged
    let resp = client
        .get(&format!("{}/api/changes", base_url))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let sessions = body["sessions"].as_array().unwrap();
    let session = sessions
        .iter()
        .find(|s| s["name"].as_str() == Some("api-session"))
        .expect("Should find api-session");
    assert_eq!(session["status"], "merged");

    let _ = server.kill();
}
