use std::collections::HashMap;
use tracing::warn;

use crate::jj::Jj;
use crate::review::Thread;

#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<HunkLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HunkLine {
    Context,
    Add,
    Delete,
}

/// Result of mapping a thread's position through a diff
#[derive(Debug, Clone)]
pub struct MappedPosition {
    pub line_start: usize,
    pub line_end: usize,
    pub is_deleted: bool,
}

/// Parse hunks for a single file from a git-format unified diff.
/// The diff_text should be the raw output of `jj diff --git` (may contain multiple files).
/// Returns hunks only for the specified file.
pub fn parse_file_hunks(diff_text: &str, target_file: &str) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    let mut in_target_file = false;
    let mut current_hunk: Option<Hunk> = None;

    for line in diff_text.lines() {
        if line.starts_with("diff --git") {
            // Flush any current hunk
            if let Some(hunk) = current_hunk.take() {
                if in_target_file {
                    hunks.push(hunk);
                }
            }
            // Check if this diff section is for our target file
            // Format: "diff --git a/path b/path"
            in_target_file = line.ends_with(&format!(" b/{}", target_file));
        } else if line.starts_with("@@") && in_target_file {
            // Flush previous hunk
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            // Parse hunk header: @@ -old_start,old_count +new_start,new_count @@
            if let Some(hunk) = parse_hunk_header(line) {
                current_hunk = Some(hunk);
            }
        } else if let Some(ref mut hunk) = current_hunk {
            if in_target_file {
                if line.starts_with('+') {
                    hunk.lines.push(HunkLine::Add);
                } else if line.starts_with('-') {
                    hunk.lines.push(HunkLine::Delete);
                } else if line.starts_with(' ') || line.is_empty() {
                    hunk.lines.push(HunkLine::Context);
                }
                // Skip other lines (e.g., "\ No newline at end of file")
            }
        }
    }

    // Flush final hunk
    if let Some(hunk) = current_hunk {
        if in_target_file {
            hunks.push(hunk);
        }
    }

    hunks
}

fn parse_hunk_header(line: &str) -> Option<Hunk> {
    // @@ -old_start,old_count +new_start,new_count @@
    // or @@ -old_start +new_start,new_count @@ (count defaults to 1)
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }

    let old_part = parts[1].trim_start_matches('-');
    let new_part = parts[2].trim_start_matches('+');

    let (old_start, old_count) = parse_range(old_part)?;
    let (new_start, new_count) = parse_range(new_part)?;

    Some(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    if let Some((start, count)) = s.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        Some((s.parse().ok()?, 1))
    }
}

/// Result of mapping a single line through hunks.
#[derive(Debug, Clone, PartialEq)]
pub struct LineMapping {
    pub new_line: usize,
    pub was_deleted: bool,
}

/// Map an old line number through hunks to find its new position.
/// If the line was deleted, returns the nearest surviving line (scanning forward,
/// then backward) with `was_deleted = true`.
pub fn map_line(old_line: usize, hunks: &[Hunk]) -> LineMapping {
    let mut offset: isize = 0;

    for hunk in hunks {
        let hunk_old_end = hunk.old_start + hunk.old_count;

        // Line is before this hunk — apply accumulated offset
        if old_line < hunk.old_start {
            return LineMapping {
                new_line: (old_line as isize + offset) as usize,
                was_deleted: false,
            };
        }

        // Line is inside this hunk — walk through hunk lines
        if old_line < hunk_old_end {
            let mut old_pos = hunk.old_start;
            let mut new_pos = hunk.new_start;
            let mut last_surviving = None;

            for hunk_line in &hunk.lines {
                match hunk_line {
                    HunkLine::Context => {
                        if old_pos == old_line {
                            return LineMapping { new_line: new_pos, was_deleted: false };
                        }
                        last_surviving = Some(new_pos);
                        old_pos += 1;
                        new_pos += 1;
                    }
                    HunkLine::Delete => {
                        if old_pos == old_line {
                            let anchor = find_nearest_surviving(hunk, old_line);
                            return LineMapping {
                                new_line: anchor,
                                was_deleted: true,
                            };
                        }
                        old_pos += 1;
                    }
                    HunkLine::Add => {
                        new_pos += 1;
                    }
                }
            }

            // Line wasn't found in the hunk (shouldn't happen with well-formed diffs)
            // Fall back to last surviving line or hunk start
            return LineMapping {
                new_line: last_surviving.unwrap_or(hunk.new_start),
                was_deleted: true,
            };
        }

        // Line is after this hunk — accumulate offset
        offset += hunk.new_count as isize - hunk.old_count as isize;
    }

    // Line is after all hunks
    LineMapping {
        new_line: (old_line as isize + offset) as usize,
        was_deleted: false,
    }
}

/// Find the nearest surviving new-file line for a deleted old-file line within a hunk.
/// Scans forward first (next context line), then backward.
fn find_nearest_surviving(hunk: &Hunk, deleted_old_line: usize) -> usize {
    let mut old_pos = hunk.old_start;
    let mut new_pos = hunk.new_start;

    // Track the last new_pos we saw before reaching the deleted line
    let mut last_before: Option<usize> = None;
    let mut reached_target = false;

    for hunk_line in &hunk.lines {
        match hunk_line {
            HunkLine::Context => {
                if reached_target {
                    return new_pos; // First surviving line after deletion
                }
                last_before = Some(new_pos);
                old_pos += 1;
                new_pos += 1;
            }
            HunkLine::Delete => {
                if old_pos == deleted_old_line {
                    reached_target = true;
                }
                old_pos += 1;
            }
            HunkLine::Add => {
                if reached_target {
                    // An added line right after the deletion — anchor here
                    return new_pos;
                }
                new_pos += 1;
            }
        }
    }

    // No surviving line found after; use the last one before, or hunk start
    last_before.unwrap_or(hunk.new_start.max(1))
}

/// Map all threads to their positions at the target commit.
/// Groups by file to avoid redundant diffs.
pub fn map_all_threads(
    jj: &Jj,
    threads: &[Thread],
    target_commit: &str,
) -> HashMap<String, MappedPosition> {
    let mut results = HashMap::new();

    // Group threads by (file, created_at_commit)
    let mut groups: HashMap<(String, String), Vec<&Thread>> = HashMap::new();

    for thread in threads {
        let commit = match &thread.created_at_commit {
            Some(c) if c != target_commit => c.clone(),
            _ => {
                // No mapping needed — use stored positions
                results.insert(
                    thread.id.clone(),
                    MappedPosition {
                        line_start: thread.line_start,
                        line_end: thread.line_end,
                        is_deleted: false,
                    },
                );
                continue;
            }
        };

        groups
            .entry((thread.file.clone(), commit))
            .or_default()
            .push(thread);
    }

    // For each unique (file, commit) pair, run one diff and map all threads
    for ((file, from_commit), group_threads) in &groups {
        let diff_text = match jj.diff_raw_between(from_commit, target_commit, &file) {
            Ok(text) => text,
            Err(e) => {
                warn!("Failed to get diff for {} from {} to {}: {}", file, from_commit, target_commit, e);
                // If diff fails (e.g., file deleted), mark all threads as deleted
                for thread in group_threads {
                    results.insert(
                        thread.id.clone(),
                        MappedPosition {
                            line_start: thread.line_start,
                            line_end: thread.line_end,
                            is_deleted: true,
                        },
                    );
                }
                continue;
            }
        };

        // Check if the diff is empty (no changes to this file)
        if diff_text.trim().is_empty() {
            for thread in group_threads {
                results.insert(
                    thread.id.clone(),
                    MappedPosition {
                        line_start: thread.line_start,
                        line_end: thread.line_end,
                        is_deleted: false,
                    },
                );
            }
            continue;
        }

        let hunks = parse_file_hunks(&diff_text, &file);

        // If no hunks found but diff text wasn't empty, it might be a file deletion
        if hunks.is_empty() && diff_text.contains("deleted file") {
            for thread in group_threads {
                results.insert(
                    thread.id.clone(),
                    MappedPosition {
                        line_start: thread.line_start,
                        line_end: thread.line_end,
                        is_deleted: true,
                    },
                );
            }
            continue;
        }

        for thread in group_threads {
            let mapped_start = map_line(thread.line_start, &hunks);
            let mapped_end = map_line(thread.line_end, &hunks);

            let is_deleted = mapped_start.was_deleted || mapped_end.was_deleted;

            results.insert(
                thread.id.clone(),
                MappedPosition {
                    line_start: mapped_start.new_line,
                    line_end: mapped_end.new_line,
                    is_deleted,
                },
            );
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hunk_header() {
        let hunk = parse_hunk_header("@@ -10,5 +12,7 @@ some context").unwrap();
        assert_eq!(hunk.old_start, 10);
        assert_eq!(hunk.old_count, 5);
        assert_eq!(hunk.new_start, 12);
        assert_eq!(hunk.new_count, 7);
    }

    #[test]
    fn test_parse_hunk_header_no_count() {
        let hunk = parse_hunk_header("@@ -1 +1,3 @@").unwrap();
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.old_count, 1);
        assert_eq!(hunk.new_start, 1);
        assert_eq!(hunk.new_count, 3);
    }

    #[test]
    fn test_parse_file_hunks() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -5,3 +5,5 @@ fn main() {
     let x = 1;
+    let y = 2;
+    let z = 3;
     let a = 4;
     let b = 5;
";
        let hunks = parse_file_hunks(diff, "src/main.rs");
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 5);
        assert_eq!(hunks[0].old_count, 3);
        assert_eq!(hunks[0].new_start, 5);
        assert_eq!(hunks[0].new_count, 5);
    }

    fn at(line: usize) -> LineMapping {
        LineMapping { new_line: line, was_deleted: false }
    }

    fn deleted(line: usize) -> LineMapping {
        LineMapping { new_line: line, was_deleted: true }
    }

    fn hunks(diff: &str) -> Vec<Hunk> {
        parse_file_hunks(diff, "f.rs")
    }

    #[test]
    fn test_map_line_before_hunk() {
        // 2 lines inserted after line 10 — line 5 is untouched
        let h = hunks("\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -10,3 +10,5 @@
 line 10
+new a
+new b
 line 11
 line 12
");
        assert_eq!(map_line(5, &h), at(5));
    }

    #[test]
    fn test_map_line_after_hunk() {
        // 2 lines inserted after line 10 — line 20 shifts to 22
        let h = hunks("\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -10,3 +10,5 @@
 line 10
+new a
+new b
 line 11
 line 12
");
        assert_eq!(map_line(20, &h), at(22));
    }

    #[test]
    fn test_map_line_deleted_anchors_backward() {
        // Lines 11-12 deleted, line 10 survives as context
        let h = hunks("\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -10,3 +10,1 @@
 line 10
-line 11
-line 12
");
        assert_eq!(map_line(11, &h), deleted(10));
        assert_eq!(map_line(12, &h), deleted(10));
        assert_eq!(map_line(10, &h), at(10));
    }

    #[test]
    fn test_map_line_deleted_anchors_forward() {
        // Lines 10-11 deleted, line 12 survives as context
        let h = hunks("\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -10,3 +10,1 @@
-line 10
-line 11
 line 12
");
        assert_eq!(map_line(10, &h), deleted(10));
        assert_eq!(map_line(11, &h), deleted(10));
    }

    #[test]
    fn test_map_line_deleted_anchors_to_replacement() {
        // Lines 10-11 deleted, replaced by 3 new lines
        let h = hunks("\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -10,2 +10,3 @@
-old line 10
-old line 11
+new a
+new b
+new c
");
        assert_eq!(map_line(10, &h), deleted(10));
        assert_eq!(map_line(11, &h), deleted(10));
    }

    #[test]
    fn test_map_line_deleted_all_lines_in_hunk() {
        // All 3 lines in the hunk deleted, nothing added
        let h = hunks("\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -10,3 +10,0 @@
-line 10
-line 11
-line 12
");
        assert_eq!(map_line(10, &h), deleted(10));
        assert_eq!(map_line(12, &h), deleted(10));
    }

    #[test]
    fn test_map_line_context_inside_hunk() {
        // 2 lines added between context lines 10 and 11
        let h = hunks("\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -10,2 +10,4 @@
 line 10
+new a
+new b
 line 11
");
        assert_eq!(map_line(10, &h), at(10));
        assert_eq!(map_line(11, &h), at(13));
    }

    #[test]
    fn test_map_line_multiple_hunks() {
        // First hunk: 2 adds after line 5. Second hunk: delete lines 21-22
        let h = hunks("\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -5,2 +5,4 @@
 line 5
+new a
+new b
 line 6
@@ -20,3 +22,1 @@
 line 20
-line 21
-line 22
");
        // Before first hunk
        assert_eq!(map_line(3, &h), at(3));
        // Between hunks: offset +2
        assert_eq!(map_line(10, &h), at(12));
        // Deleted line 21 anchors to context line 20 (new pos 22)
        assert_eq!(map_line(21, &h), deleted(22));
        // After both hunks: +2 - 2 = 0
        assert_eq!(map_line(30, &h), at(30));
    }

    #[test]
    fn test_no_hunks() {
        assert_eq!(map_line(42, &[]), at(42));
    }

    #[test]
    fn test_parse_file_hunks_multi_file_diff() {
        // Verify we only get hunks for the target file
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 line1
+inserted
 line2
 line3
diff --git a/bar.rs b/bar.rs
--- a/bar.rs
+++ b/bar.rs
@@ -1,2 +1,3 @@
 a
+b
 c
";
        let foo_hunks = parse_file_hunks(diff, "foo.rs");
        assert_eq!(foo_hunks.len(), 1);
        assert_eq!(foo_hunks[0].old_count, 3);
        assert_eq!(foo_hunks[0].new_count, 4);

        let bar_hunks = parse_file_hunks(diff, "bar.rs");
        assert_eq!(bar_hunks.len(), 1);
        assert_eq!(bar_hunks[0].old_count, 2);
        assert_eq!(bar_hunks[0].new_count, 3);

        let missing = parse_file_hunks(diff, "nope.rs");
        assert!(missing.is_empty());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::review::{Thread, ThreadStatus, Comment, Author};
    use std::process::Command;
    use tempfile::TempDir;

    /// Helper to create a temp jj repo and return (dir, Jj)
    fn make_jj_repo() -> (TempDir, Jj) {
        let dir = TempDir::new().unwrap();
        let path = dir.path();

        Command::new("jj")
            .args(["git", "init"])
            .current_dir(path)
            .output()
            .expect("jj git init failed");

        let jj = Jj::new(path);
        (dir, jj)
    }

    fn jj_cmd(dir: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("jj")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        if !output.status.success() {
            panic!(
                "jj {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        String::from_utf8(output.stdout).unwrap()
    }

    fn get_commit_id(dir: &std::path::Path) -> String {
        jj_cmd(dir, &["log", "--no-graph", "-r", "@", "-T", "commit_id"])
            .trim()
            .to_string()
    }

    fn make_thread(id: &str, file: &str, start: usize, end: usize, commit: &str) -> Thread {
        Thread {
            id: id.to_string(),
            file: file.to_string(),
            line_start: start,
            line_end: end,
            status: ThreadStatus::Open,
            comments: vec![Comment {
                author: Author::User,
                text: "test".to_string(),
                timestamp: chrono::Utc::now(),
            }],
            created_at_commit: Some(commit.to_string()),
            created_at_revision: Some(1),
            display_line_start: None,
            display_line_end: None,
            is_displaced: false,
            is_deleted: false,
        }
    }

    #[test]
    fn test_lines_shift_down() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        // Create file with 10 lines
        let content: String = (1..=10).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(path.join("test.rs"), &content).unwrap();
        jj_cmd(path, &["describe", "-m", "initial"]);

        let commit1 = get_commit_id(path);

        // New change: insert 3 lines at the top
        jj_cmd(path, &["new", "-m", "add lines at top"]);
        let mut new_content = "new1\nnew2\nnew3\n".to_string();
        new_content.push_str(&content);
        std::fs::write(path.join("test.rs"), &new_content).unwrap();

        let commit2 = get_commit_id(path);

        // Thread was on line 5 at commit1 → should be at line 8 now
        let threads = vec![make_thread("t1", "test.rs", 5, 5, &commit1)];
        let mapped = map_all_threads(&jj, &threads, &commit2);

        let pos = &mapped["t1"];
        assert_eq!(pos.line_start, 8);
        assert_eq!(pos.line_end, 8);
        assert!(!pos.is_deleted);
    }

    #[test]
    fn test_lines_shift_up() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        // Create file with 10 lines
        let content: String = (1..=10).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(path.join("test.rs"), &content).unwrap();
        jj_cmd(path, &["describe", "-m", "initial"]);

        let commit1 = get_commit_id(path);

        // New change: delete lines 2-3
        jj_cmd(path, &["new", "-m", "delete lines"]);
        let new_content: String = (1..=10)
            .filter(|i| *i != 2 && *i != 3)
            .map(|i| format!("line {}\n", i))
            .collect();
        std::fs::write(path.join("test.rs"), &new_content).unwrap();

        let commit2 = get_commit_id(path);

        // Thread was on line 7 at commit1 → should be at line 5 now
        let threads = vec![make_thread("t1", "test.rs", 7, 7, &commit1)];
        let mapped = map_all_threads(&jj, &threads, &commit2);

        let pos = &mapped["t1"];
        assert_eq!(pos.line_start, 5);
        assert_eq!(pos.line_end, 5);
        assert!(!pos.is_deleted);
    }

    #[test]
    fn test_commented_line_deleted() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        let content: String = (1..=10).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(path.join("test.rs"), &content).unwrap();
        jj_cmd(path, &["describe", "-m", "initial"]);

        let commit1 = get_commit_id(path);

        // New change: delete line 5
        jj_cmd(path, &["new", "-m", "delete line 5"]);
        let new_content: String = (1..=10)
            .filter(|i| *i != 5)
            .map(|i| format!("line {}\n", i))
            .collect();
        std::fs::write(path.join("test.rs"), &new_content).unwrap();

        let commit2 = get_commit_id(path);

        let threads = vec![make_thread("t1", "test.rs", 5, 5, &commit1)];
        let mapped = map_all_threads(&jj, &threads, &commit2);

        let pos = &mapped["t1"];
        assert!(pos.is_deleted);
        // Deleted line anchors to nearest surviving line (line 6 becomes line 5)
        assert_eq!(pos.line_start, 5);
    }

    #[test]
    fn test_no_change_same_commit() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        let content = "line 1\nline 2\nline 3\n";
        std::fs::write(path.join("test.rs"), content).unwrap();
        jj_cmd(path, &["describe", "-m", "initial"]);

        let commit1 = get_commit_id(path);

        // Same commit → positions should be unchanged
        let threads = vec![make_thread("t1", "test.rs", 2, 3, &commit1)];
        let mapped = map_all_threads(&jj, &threads, &commit1);

        let pos = &mapped["t1"];
        assert_eq!(pos.line_start, 2);
        assert_eq!(pos.line_end, 3);
        assert!(!pos.is_deleted);
    }

    #[test]
    fn test_multiple_hunks() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        // Create a file with 20 lines
        let content: String = (1..=20).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(path.join("test.rs"), &content).unwrap();
        jj_cmd(path, &["describe", "-m", "initial"]);

        let commit1 = get_commit_id(path);

        // New change: insert 2 lines after line 3, and delete line 15
        jj_cmd(path, &["new", "-m", "multi-hunk changes"]);
        let mut lines: Vec<String> = (1..=20).map(|i| format!("line {}", i)).collect();
        lines.insert(3, "new_a".to_string());
        lines.insert(4, "new_b".to_string());
        // line 15 is now at index 16 (after inserting 2 lines)
        lines.remove(16);
        let new_content = lines.join("\n") + "\n";
        std::fs::write(path.join("test.rs"), &new_content).unwrap();

        let commit2 = get_commit_id(path);

        let threads = vec![
            make_thread("t1", "test.rs", 1, 1, &commit1),  // before first hunk
            make_thread("t2", "test.rs", 10, 10, &commit1), // between hunks, should shift +2
            make_thread("t3", "test.rs", 15, 15, &commit1), // the deleted line
            make_thread("t4", "test.rs", 20, 20, &commit1), // after both hunks, net shift +1
        ];
        let mapped = map_all_threads(&jj, &threads, &commit2);

        assert_eq!(mapped["t1"].line_start, 1);
        assert!(!mapped["t1"].is_deleted);

        assert_eq!(mapped["t2"].line_start, 12);
        assert!(!mapped["t2"].is_deleted);

        assert!(mapped["t3"].is_deleted);

        assert_eq!(mapped["t4"].line_start, 21);
        assert!(!mapped["t4"].is_deleted);
    }

    #[test]
    fn test_file_deleted() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        std::fs::write(path.join("test.rs"), "line 1\nline 2\n").unwrap();
        jj_cmd(path, &["describe", "-m", "initial"]);

        let commit1 = get_commit_id(path);

        // New change: delete the file
        jj_cmd(path, &["new", "-m", "delete file"]);
        std::fs::remove_file(path.join("test.rs")).unwrap();

        let commit2 = get_commit_id(path);

        let threads = vec![make_thread("t1", "test.rs", 1, 2, &commit1)];
        let mapped = map_all_threads(&jj, &threads, &commit2);

        let pos = &mapped["t1"];
        assert!(pos.is_deleted);
    }

    #[test]
    fn test_thread_without_created_at_commit() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        std::fs::write(path.join("test.rs"), "line 1\n").unwrap();
        jj_cmd(path, &["describe", "-m", "initial"]);

        let commit1 = get_commit_id(path);

        // Thread without created_at_commit (old threads)
        let threads = vec![Thread {
            id: "t1".to_string(),
            file: "test.rs".to_string(),
            line_start: 1,
            line_end: 1,
            status: ThreadStatus::Open,
            comments: vec![],
            created_at_commit: None,
            created_at_revision: None,
            display_line_start: None,
            display_line_end: None,
            is_displaced: false,
            is_deleted: false,
        }];

        let mapped = map_all_threads(&jj, &threads, &commit1);
        let pos = &mapped["t1"];
        assert_eq!(pos.line_start, 1);
        assert!(!pos.is_deleted);
    }
}
