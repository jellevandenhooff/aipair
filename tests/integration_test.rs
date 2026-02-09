//! Integration tests for aipair
//!
//! These tests spin up a real server against a temporary jj repository
//! and verify the full flow works end-to-end.

use reqwest::Client;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;

struct TestHarness {
    temp_dir: TempDir,
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
            temp_dir,
            server,
            client,
            base_url,
        }
    }

    fn repo_path(&self) -> &std::path::Path {
        self.temp_dir.path()
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

    // Create working change
    Command::new("jj")
        .args(["new", "-m", "Working change"])
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
