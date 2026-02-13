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
    #[serde(default = "default_base_bookmark")]
    pub base_bookmark: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub pushes: Vec<PushEvent>,
    #[serde(default)]
    pub changes: Vec<String>,
}

fn default_base_bookmark() -> String {
    "main".to_string()
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
    // Walk up from cwd looking for .aipair/session.json
    let cwd = std::env::current_dir()?;
    let mut dir = cwd.as_path();
    loop {
        let marker_path = dir.join(".aipair/session.json");
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

pub fn session_new(name: &str, base_bookmark: &str) -> Result<()> {
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

    // Get current base change_id
    let base_change_id = jj
        .get_bookmark(base_bookmark)
        .with_context(|| format!("Failed to find '{base_bookmark}' bookmark"))?
        .with_context(|| format!("No '{base_bookmark}' bookmark found"))?;

    // Clone
    let clone_rel = format!(".aipair/sessions/{name}/repo");
    let clone_path = repo_path.join(&clone_rel);
    if clone_path.exists() {
        anyhow::bail!("Clone directory already exists: {}", clone_path.display());
    }

    println!("Cloning into {}...", clone_path.display());
    let clone_jj = Jj::git_clone(&repo_path, &clone_path)?;

    // The clone's WC lands on root, not main. Create a new change on top of base@origin.
    clone_jj.new_change_on(&format!("{base_bookmark}@origin"), name)?;

    // Create bookmark in clone
    let bookmark = format!("session/{name}");
    clone_jj.bookmark_create(&bookmark, "@")?;

    // Write clone marker
    let marker = CloneMarker {
        session_name: name.to_string(),
        main_repo: repo_path.to_string_lossy().to_string(),
        bookmark: bookmark.clone(),
    };
    let marker_dir = clone_path.join(".aipair");
    fs::create_dir_all(&marker_dir)?;
    let marker_path = marker_dir.join("session.json");
    fs::write(&marker_path, serde_json::to_string_pretty(&marker)?)?;

    // Save session metadata
    let session = Session {
        name: name.to_string(),
        clone_path: clone_rel.clone(),
        bookmark: bookmark.clone(),
        base_change_id,
        base_bookmark: base_bookmark.to_string(),
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
    let base_ref = format!("{}@origin..@", session.base_bookmark);
    session.changes = jj.query_change_ids(&base_ref)?;

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

    // Load session metadata to get base_bookmark
    let main_repo_path = PathBuf::from(&marker.main_repo);
    let store = SessionStore::new(&main_repo_path);
    let session = store
        .get(&marker.session_name)?
        .context("Session metadata not found")?;
    let base_ref = format!("{}@origin", session.base_bookmark);

    println!("Fetching from origin...");
    let fetch_output = jj.git_fetch()?;
    if !fetch_output.is_empty() {
        print!("{fetch_output}");
    }

    // Rebase onto the base ref (could be main@origin or another session's bookmark)
    println!("Rebasing onto {base_ref}...");
    let rebase_output = jj.rebase("@", &base_ref)?;
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

    println!(
        "Moving {} to {bookmark} (change {})...",
        session.base_bookmark,
        &session_tip[..12]
    );
    jj.move_bookmark(&session.base_bookmark, &session_tip)?;

    // Delete session bookmark
    jj.bookmark_delete(bookmark)?;

    // Update status
    session.status = SessionStatus::Merged;
    store.save(&session)?;

    // Re-parent child sessions that were stacked on this session's bookmark
    let all_sessions = store.list()?;
    for mut child in all_sessions {
        if child.status == SessionStatus::Active && child.base_bookmark == session.bookmark {
            child.base_bookmark = session.base_bookmark.clone();
            store.save(&child)?;
            println!(
                "  Re-parented session '{}' onto {}",
                child.name, child.base_bookmark
            );
        }
    }

    println!();
    println!(
        "Session '{name}' merged into {}!",
        session.base_bookmark
    );
    println!(
        "  {} now at change {}",
        session.base_bookmark,
        &session_tip[..12]
    );

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
        "{:<20} {:<8} {:<15} {:<8} {:<25}",
        "NAME", "STATUS", "BASE", "PUSHES", "LAST PUSH"
    );
    println!("{}", "-".repeat(80));

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
        // Truncate last_push to 25 chars
        let last_push_display = if last_push.len() > 25 {
            format!("{}...", &last_push[..22])
        } else {
            last_push.to_string()
        };
        println!(
            "{:<20} {:<8} {:<15} {:<8} {:<25}",
            s.name,
            status,
            s.base_bookmark,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_session(name: &str, base_bookmark: &str, status: SessionStatus) -> Session {
        Session {
            name: name.to_string(),
            clone_path: format!(".aipair/sessions/{name}/repo"),
            bookmark: format!("session/{name}"),
            base_change_id: "abc123".to_string(),
            base_bookmark: base_bookmark.to_string(),
            status,
            created_at: Utc::now(),
            pushes: Vec::new(),
            changes: Vec::new(),
        }
    }

    #[test]
    fn test_base_bookmark_defaults_to_main_on_deserialize() {
        // Simulate an old session JSON without base_bookmark
        let json = r#"{
            "name": "old-session",
            "clone_path": ".aipair/sessions/old-session/repo",
            "bookmark": "session/old-session",
            "base_change_id": "abc123",
            "status": "active",
            "created_at": "2025-01-01T00:00:00Z",
            "pushes": [],
            "changes": []
        }"#;

        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.base_bookmark, "main");
    }

    #[test]
    fn test_base_bookmark_preserved_on_roundtrip() {
        let session = make_session("child", "session/parent", SessionStatus::Active);
        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.base_bookmark, "session/parent");
    }

    #[test]
    fn test_session_store_save_and_get() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path());

        let session = make_session("test-session", "main", SessionStatus::Active);
        store.save(&session).unwrap();

        let loaded = store.get("test-session").unwrap().unwrap();
        assert_eq!(loaded.name, "test-session");
        assert_eq!(loaded.base_bookmark, "main");
        assert_eq!(loaded.status, SessionStatus::Active);
    }

    #[test]
    fn test_session_store_get_nonexistent() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path());

        assert!(store.get("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_session_store_list_sorted_by_created_at() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path());

        // Create sessions with different timestamps (save order shouldn't matter)
        let mut s2 = make_session("beta", "main", SessionStatus::Active);
        s2.created_at = Utc::now();
        store.save(&s2).unwrap();

        let mut s1 = make_session("alpha", "main", SessionStatus::Active);
        s1.created_at = s2.created_at - chrono::Duration::seconds(10);
        store.save(&s1).unwrap();

        let sessions = store.list().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "alpha");
        assert_eq!(sessions[1].name, "beta");
    }

    #[test]
    fn test_session_store_with_stacked_base_bookmark() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path());

        let parent = make_session("parent", "main", SessionStatus::Active);
        store.save(&parent).unwrap();

        let child = make_session("child", "session/parent", SessionStatus::Active);
        store.save(&child).unwrap();

        let loaded = store.get("child").unwrap().unwrap();
        assert_eq!(loaded.base_bookmark, "session/parent");
    }

    #[test]
    fn test_reparent_children_on_merge() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path());

        // Set up: parent based on main, child based on parent, grandchild based on child
        let parent = make_session("parent", "main", SessionStatus::Active);
        store.save(&parent).unwrap();

        let child = make_session("child", "session/parent", SessionStatus::Active);
        store.save(&child).unwrap();

        let grandchild = make_session("grandchild", "session/child", SessionStatus::Active);
        store.save(&grandchild).unwrap();

        // Simulate merging parent: mark as merged and re-parent children
        let mut parent = store.get("parent").unwrap().unwrap();
        parent.status = SessionStatus::Merged;
        store.save(&parent).unwrap();

        let all_sessions = store.list().unwrap();
        for mut s in all_sessions {
            if s.status == SessionStatus::Active && s.base_bookmark == parent.bookmark {
                s.base_bookmark = parent.base_bookmark.clone();
                store.save(&s).unwrap();
            }
        }

        // child should now point to main (was session/parent)
        let child = store.get("child").unwrap().unwrap();
        assert_eq!(child.base_bookmark, "main");

        // grandchild should still point to session/child (not directly affected)
        let grandchild = store.get("grandchild").unwrap().unwrap();
        assert_eq!(grandchild.base_bookmark, "session/child");
    }

    #[test]
    fn test_reparent_does_not_affect_merged_sessions() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path());

        let parent = make_session("parent", "main", SessionStatus::Active);
        store.save(&parent).unwrap();

        // A merged session that happened to be based on parent
        let mut old_child = make_session("old-child", "session/parent", SessionStatus::Merged);
        old_child.status = SessionStatus::Merged;
        store.save(&old_child).unwrap();

        // Simulate merging parent
        let mut parent = store.get("parent").unwrap().unwrap();
        parent.status = SessionStatus::Merged;
        store.save(&parent).unwrap();

        let all_sessions = store.list().unwrap();
        for mut s in all_sessions {
            if s.status == SessionStatus::Active && s.base_bookmark == parent.bookmark {
                s.base_bookmark = parent.base_bookmark.clone();
                store.save(&s).unwrap();
            }
        }

        // old-child should NOT be re-parented (it's merged, not active)
        let old_child = store.get("old-child").unwrap().unwrap();
        assert_eq!(old_child.base_bookmark, "session/parent");
    }

    #[test]
    fn test_session_list_empty() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path());
        let sessions = store.list().unwrap();
        assert!(sessions.is_empty());
    }
}

