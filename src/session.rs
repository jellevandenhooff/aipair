use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::jj::Jj;
use crate::review::{Author, ReviewStore};

// --- Data types ---

#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub clone_path: String,
    pub bookmark: String,
    pub base_change_id: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub pushes: Vec<PushEvent>,
    #[serde(default)]
    pub changes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Merged,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushEvent {
    pub summary: String,
    pub change_id: String,
    pub commit_id: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CloneMarker {
    pub session_name: String,
    pub main_repo: String,
    pub bookmark: String,
}

// --- SessionStore ---

pub struct SessionStore {
    base_path: PathBuf,
}

impl SessionStore {
    pub fn new(repo_path: &Path) -> Self {
        Self {
            base_path: repo_path.to_path_buf(),
        }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.base_path.join(".aipair").join("sessions")
    }

    fn session_file(&self, name: &str) -> PathBuf {
        self.sessions_dir().join(format!("{name}.json"))
    }

    pub fn save(&self, session: &Session) -> Result<()> {
        let dir = self.sessions_dir();
        fs::create_dir_all(&dir)?;
        let path = self.session_file(&session.name);
        let json = serde_json::to_string_pretty(session)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Option<Session>> {
        let path = self.session_file(name);
        if !path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(path)?;
        let session: Session = serde_json::from_str(&json)?;
        Ok(Some(session))
    }

    pub fn list(&self) -> Result<Vec<Session>> {
        let dir = self.sessions_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut sessions = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "json") {
                let json = fs::read_to_string(&path)?;
                let session: Session = serde_json::from_str(&json)?;
                sessions.push(session);
            }
        }
        sessions.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(sessions)
    }
}

// --- Context detection ---

pub enum SessionContext {
    MainRepo { jj: Jj, repo_path: PathBuf },
    SessionClone { jj: Jj, marker: CloneMarker },
}

pub fn detect_context() -> Result<SessionContext> {
    // Walk up from cwd looking for .aipair-session.json
    let cwd = std::env::current_dir()?;
    let mut dir = cwd.as_path();
    loop {
        let marker_path = dir.join(".aipair-session.json");
        if marker_path.exists() {
            let json = fs::read_to_string(&marker_path)?;
            let marker: CloneMarker = serde_json::from_str(&json)?;
            let jj = Jj::new(dir);
            return Ok(SessionContext::SessionClone { jj, marker });
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }

    // No marker found — treat as main repo
    let jj = Jj::discover()?;
    let repo_path = jj.repo_path().to_path_buf();
    Ok(SessionContext::MainRepo { jj, repo_path })
}

// --- Operations ---

pub fn session_new(name: &str) -> Result<()> {
    // Validate name
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("Session name must be alphanumeric with hyphens/underscores only");
    }

    let jj = Jj::discover()?;
    let repo_path = jj.repo_path().to_path_buf();
    let store = SessionStore::new(&repo_path);

    // Check for duplicate
    if store.get(name)?.is_some() {
        anyhow::bail!("Session '{name}' already exists");
    }

    // Get current main change_id for base
    let base_change_id = jj
        .get_bookmark("main")
        .context("Failed to find 'main' bookmark")?
        .context("No 'main' bookmark found — is this an aipair repo?")?;

    // Clone
    let clone_rel = format!(".aipair/sessions/{name}/repo");
    let clone_path = repo_path.join(&clone_rel);
    if clone_path.exists() {
        anyhow::bail!("Clone directory already exists: {}", clone_path.display());
    }

    println!("Cloning into {}...", clone_path.display());
    let clone_jj = Jj::git_clone(&repo_path, &clone_path)?;

    // The clone's WC lands on root, not main. Create a new change on top of main@origin.
    clone_jj.new_change_on("main@origin", name)?;

    // Create bookmark in clone
    let bookmark = format!("session/{name}");
    clone_jj.bookmark_create(&bookmark, "@")?;

    // Write clone marker
    let marker = CloneMarker {
        session_name: name.to_string(),
        main_repo: repo_path.to_string_lossy().to_string(),
        bookmark: bookmark.clone(),
    };
    let marker_path = clone_path.join(".aipair-session.json");
    fs::write(&marker_path, serde_json::to_string_pretty(&marker)?)?;

    // Save session metadata
    let session = Session {
        name: name.to_string(),
        clone_path: clone_rel.clone(),
        bookmark: bookmark.clone(),
        base_change_id,
        status: SessionStatus::Active,
        created_at: Utc::now(),
        pushes: Vec::new(),
        changes: Vec::new(),
    };
    store.save(&session)?;

    // Check for aipair mention in CLAUDE.md
    let claude_md = repo_path.join("CLAUDE.md");
    let has_aipair_mention = claude_md.exists()
        && fs::read_to_string(&claude_md)
            .map(|s| s.to_lowercase().contains("aipair"))
            .unwrap_or(false);

    println!();
    println!("Session '{name}' created!");
    println!("  Clone: {}", clone_path.display());
    println!("  Bookmark: {bookmark}");
    println!();
    println!("Next steps:");
    println!("  cd {clone_rel}");
    println!("  # make changes, then:");
    println!("  aipair push -m \"description of changes\"");

    if !has_aipair_mention {
        eprintln!();
        eprintln!("Warning: No mention of 'aipair' found in CLAUDE.md");
        eprintln!("  Run `aipair session setup-claude` to add workflow instructions.");
    }

    Ok(())
}

pub fn push(message: &str) -> Result<()> {
    let ctx = detect_context()?;
    let (jj, marker) = match ctx {
        SessionContext::SessionClone { jj, marker } => (jj, marker),
        SessionContext::MainRepo { .. } => {
            anyhow::bail!("'push' must be run from a session clone, not the main repo");
        }
    };

    // Update bookmark to point to current working copy
    jj.move_bookmark(&marker.bookmark, "@")?;

    // Check if this is the first push (bookmark doesn't exist on remote yet)
    // We detect this by checking if the session has any prior pushes
    let main_repo_path = PathBuf::from(&marker.main_repo);
    let store = SessionStore::new(&main_repo_path);
    let mut session = store
        .get(&marker.session_name)?
        .context("Session metadata not found in main repo")?;
    let allow_new = session.pushes.is_empty();

    println!("Pushing {}...", marker.bookmark);
    let push_output = jj.git_push_bookmark(&marker.bookmark, allow_new)?;
    if !push_output.is_empty() {
        print!("{push_output}");
    }

    // Record push event
    let change = jj.get_change("@")?;
    session.pushes.push(PushEvent {
        summary: message.to_string(),
        change_id: change.change_id,
        commit_id: change.commit_id,
        timestamp: Utc::now(),
    });

    // Record all session change_ids (from clone's perspective)
    session.changes = jj.query_change_ids("main@origin..@")?;

    store.save(&session)?;

    println!("Pushed! Summary: {message}");
    Ok(())
}

pub fn pull() -> Result<()> {
    let ctx = detect_context()?;
    let (jj, marker) = match ctx {
        SessionContext::SessionClone { jj, marker } => (jj, marker),
        SessionContext::MainRepo { .. } => {
            anyhow::bail!("'pull' must be run from a session clone, not the main repo");
        }
    };

    println!("Fetching from origin...");
    let fetch_output = jj.git_fetch()?;
    if !fetch_output.is_empty() {
        print!("{fetch_output}");
    }

    // Check if main moved — try rebasing onto latest main
    // In a clone, main is only available as main@origin
    println!("Rebasing onto main@origin...");
    let rebase_output = jj.rebase("@", "main@origin")?;
    if !rebase_output.is_empty() {
        print!("{rebase_output}");
    }

    // Check for conflicts
    let change = jj.get_change("@")?;
    if change.conflict {
        println!();
        println!("WARNING: Rebase produced conflicts! Resolve them before pushing.");
    } else {
        println!("Up to date, no conflicts.");
    }

    // Also update the bookmark in the clone after rebase
    jj.move_bookmark(&marker.bookmark, "@")?;

    Ok(())
}

pub fn session_merge(name: &str) -> Result<()> {
    let ctx = detect_context()?;
    let (jj, repo_path) = match ctx {
        SessionContext::MainRepo { jj, repo_path } => (jj, repo_path),
        SessionContext::SessionClone { .. } => {
            anyhow::bail!("'session merge' must be run from the main repo, not a session clone");
        }
    };

    let store = SessionStore::new(&repo_path);
    let mut session = store
        .get(name)?
        .context(format!("Session '{name}' not found"))?;

    if session.status != SessionStatus::Active {
        anyhow::bail!("Session '{name}' is not active (status: {:?})", session.status);
    }

    // Fetch to make sure we have latest from the clone's pushes
    println!("Fetching latest...");
    let _ = jj.git_fetch();

    // Move main bookmark to the session bookmark tip
    let bookmark = &session.bookmark;
    let session_tip = jj
        .get_bookmark(bookmark)?
        .context(format!("Bookmark '{bookmark}' not found — was it pushed?"))?;

    println!("Moving main to {bookmark} (change {})...", &session_tip[..12]);
    jj.move_bookmark("main", &session_tip)?;

    // Delete session bookmark
    jj.bookmark_delete(bookmark)?;

    // Update status
    session.status = SessionStatus::Merged;
    store.save(&session)?;

    println!();
    println!("Session '{name}' merged into main!");
    println!("  main now at change {}", &session_tip[..12]);

    Ok(())
}

pub fn session_list() -> Result<()> {
    let ctx = detect_context()?;
    let repo_path = match ctx {
        SessionContext::MainRepo { repo_path, .. } => repo_path,
        SessionContext::SessionClone { marker, .. } => PathBuf::from(&marker.main_repo),
    };

    let store = SessionStore::new(&repo_path);
    let sessions = store.list()?;

    if sessions.is_empty() {
        println!("No sessions.");
        return Ok(());
    }

    println!(
        "{:<20} {:<8} {:<8} {:<30}",
        "NAME", "STATUS", "PUSHES", "LAST PUSH"
    );
    println!("{}", "-".repeat(70));

    for s in &sessions {
        let status = match s.status {
            SessionStatus::Active => "active",
            SessionStatus::Merged => "merged",
        };
        let last_push = s
            .pushes
            .last()
            .map(|p| p.summary.as_str())
            .unwrap_or("-");
        // Truncate last_push to 30 chars
        let last_push_display = if last_push.len() > 30 {
            format!("{}...", &last_push[..27])
        } else {
            last_push.to_string()
        };
        println!(
            "{:<20} {:<8} {:<8} {:<30}",
            s.name,
            status,
            s.pushes.len(),
            last_push_display,
        );
    }

    Ok(())
}

pub fn status() -> Result<()> {
    let ctx = detect_context()?;
    match ctx {
        SessionContext::MainRepo { repo_path, .. } => {
            let store = SessionStore::new(&repo_path);
            let sessions = store.list()?;
            let active: Vec<_> = sessions
                .iter()
                .filter(|s| s.status == SessionStatus::Active)
                .collect();
            if active.is_empty() {
                println!("No active sessions.");
            } else {
                println!("Active sessions:");
                for s in &active {
                    let push_count = s.pushes.len();
                    println!("  {} ({} pushes)", s.name, push_count);
                }
            }
        }
        SessionContext::SessionClone { jj, marker } => {
            println!("Session: {}", marker.session_name);
            println!("Bookmark: {}", marker.bookmark);
            println!("Main repo: {}", marker.main_repo);

            let main_repo_path = PathBuf::from(&marker.main_repo);
            let store = SessionStore::new(&main_repo_path);
            if let Some(session) = store.get(&marker.session_name)? {
                println!("Pushes: {}", session.pushes.len());
                if let Some(last) = session.pushes.last() {
                    println!("Last push: {}", last.summary);
                }
            }

            // Show current change
            let change = jj.get_change("@")?;
            println!();
            println!("Current change: {}", &change.change_id[..12]);
            if !change.description.is_empty() {
                println!("  {}", change.description);
            }
            if change.conflict {
                println!("  WARNING: has conflicts");
            }
        }
    }

    Ok(())
}

pub fn feedback() -> Result<()> {
    let ctx = detect_context()?;
    let (_jj, marker) = match ctx {
        SessionContext::SessionClone { jj, marker } => (jj, marker),
        SessionContext::MainRepo { .. } => {
            anyhow::bail!("'feedback' must be run from a session clone, not the main repo");
        }
    };

    let main_repo_path = PathBuf::from(&marker.main_repo);
    let main_jj = Jj::new(&main_repo_path);
    let store = ReviewStore::new(&main_repo_path);
    let session_store = SessionStore::new(&main_repo_path);

    let session = session_store
        .get(&marker.session_name)?
        .context("Session metadata not found in main repo")?;

    let change_ids: HashSet<String> = session.changes.into_iter().collect();
    if change_ids.is_empty() {
        println!("No changes tracked yet. Push first with `aipair push -m \"...\"`");
        return Ok(());
    }

    let reviews = store.list_with_open_threads(Some(&change_ids))?;
    if reviews.is_empty() {
        println!("No pending feedback.");
        return Ok(());
    }

    let output = crate::mcp::format_pending_feedback(&main_jj, reviews);
    print!("{output}");
    Ok(())
}

pub fn respond(change_id: &str, thread_id: &str, message: &str, resolve: bool) -> Result<()> {
    let ctx = detect_context()?;
    let marker = match ctx {
        SessionContext::SessionClone { marker, .. } => marker,
        SessionContext::MainRepo { .. } => {
            anyhow::bail!("'respond' must be run from a session clone, not the main repo");
        }
    };

    let main_repo_path = PathBuf::from(&marker.main_repo);
    let store = ReviewStore::new(&main_repo_path);

    store.reply_to_thread(change_id, thread_id, Author::Claude, message)?;

    if resolve {
        store.resolve_thread(change_id, thread_id)?;
    }

    let status = if resolve { " and resolved" } else { "" };
    println!("Responded to thread {}{status}.", &thread_id[..8.min(thread_id.len())]);
    Ok(())
}

pub fn session_setup_claude() -> Result<()> {
    let jj = Jj::discover()?;
    let repo_path = jj.repo_path().to_path_buf();
    let claude_md = repo_path.join("CLAUDE.md");

    let section = r#"
## Session Workflow (aipair)

### Commands (run from session clone directory)
- `aipair push -m "summary"` — push changes for review
- `aipair pull` — pull latest main and rebase
- `aipair feedback` — show pending review comments
- `aipair respond <change-id> <thread-id> "message" [--resolve]` — reply to a review thread
- `aipair status` — show session info

### Workflow
1. Make changes, then push: `aipair push -m "description"`
2. Check for feedback: `aipair feedback`
3. Address comments, respond: `aipair respond <change-id> <thread-id> "Fixed" --resolve`
4. Push again: `aipair push -m "Address feedback"`
5. Repeat until all threads resolved
"#;

    if claude_md.exists() {
        let content = fs::read_to_string(&claude_md)?;
        if content.to_lowercase().contains("session workflow (aipair)") {
            println!("CLAUDE.md already contains session workflow instructions.");
            return Ok(());
        }
        let mut new_content = content;
        new_content.push_str(section);
        fs::write(&claude_md, new_content)?;
    } else {
        fs::write(&claude_md, format!("# Project Guidelines\n{section}"))?;
    }

    println!("Added session workflow instructions to CLAUDE.md");
    Ok(())
}
