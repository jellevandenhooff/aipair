use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct Change {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    pub author: String,
    pub timestamp: String,
    pub empty: bool,
    pub conflict: bool,
    pub is_working_copy: bool,
    pub parent_change_ids: Vec<String>,
}

/// Internal struct for deserializing jj's JSON output
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JjChange {
    change_id: String,
    commit_id: String,
    description: String,
    author: JjSignature,
    committer: JjSignature,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JjSignature {
    email: String,
    timestamp: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct Diff {
    pub change_id: String,
    pub base: String,
    pub files: Vec<FileDiff>,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct FileDiff {
    pub path: String,
    pub status: FileStatus,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
}

pub struct Jj {
    repo_path: std::path::PathBuf,
}

impl Jj {
    pub fn new(repo_path: impl AsRef<Path>) -> Self {
        Self {
            repo_path: repo_path.as_ref().to_path_buf(),
        }
    }

    pub fn discover() -> Result<Self> {
        let output = Command::new("jj")
            .args(["root"])
            .output()
            .context("Failed to run jj root")?;

        if !output.status.success() {
            anyhow::bail!(
                "Not in a jj repository: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let root = String::from_utf8(output.stdout)?.trim().to_string();
        Ok(Self::new(root))
    }

    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    /// List recent changes across all heads
    pub fn log(&self, limit: usize) -> Result<Vec<Change>> {
        // Use json(self) for proper escaping of description, append empty flag with tab separator
        // Walk from all visible heads (not just @) to capture changes across branches/topics
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args([
                "log",
                "--no-graph",
                "-r",
                &format!("ancestors(visible_heads(), {limit})"),
                "-T",
                r#"json(self) ++ "\t" ++ empty ++ "\t" ++ conflict ++ "\t" ++ self.current_working_copy() ++ "\t" ++ parents.map(|c| c.change_id()).join(",") ++ "\n""#,
            ])
            .output()
            .context("Failed to run jj log")?;

        if !output.status.success() {
            anyhow::bail!("jj log failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let mut changes = Vec::new();

        for line in stdout.lines() {
            if line.trim().is_empty() {
                continue;
            }

            // Parse "json\tempty\tconflict\tis_wc\tparents" format
            let parts: Vec<&str> = line.rsplitn(5, '\t').collect();
            if parts.len() < 5 {
                continue;
            }
            let parents_str = parts[0];
            let is_wc_str = parts[1];
            let conflict_str = parts[2];
            let empty_str = parts[3];
            let json_str = parts[4];

            let jj_change: JjChange = serde_json::from_str(json_str)
                .with_context(|| format!("Failed to parse jj log output: {json_str}"))?;

            // Skip the root commit (all z's) - it has no parent and can't be diffed
            if jj_change.change_id.chars().all(|c| c == 'z') {
                continue;
            }

            let parent_change_ids: Vec<String> = if parents_str.is_empty() {
                Vec::new()
            } else {
                parents_str.split(',').map(|s| s.to_string()).collect()
            };

            changes.push(Change {
                change_id: jj_change.change_id,
                commit_id: jj_change.commit_id,
                description: jj_change.description.trim_end().to_string(),
                author: jj_change.author.email,
                timestamp: jj_change.committer.timestamp,
                empty: empty_str == "true",
                conflict: conflict_str == "true",
                is_working_copy: is_wc_str == "true",
                parent_change_ids,
            });
        }

        Ok(changes)
    }

    /// Get diff for a change (compared to its parent by default)
    pub fn diff(&self, change_id: &str, base: Option<&str>) -> Result<Diff> {
        let default_base = format!("{change_id}-");
        let base = base.unwrap_or(&default_base);
        let raw = self.diff_raw(change_id, base)?;
        let files = self.diff_stat(change_id, base)?;

        Ok(Diff {
            change_id: change_id.to_string(),
            base: base.to_string(),
            files,
            raw,
        })
    }

    fn diff_raw(&self, change_id: &str, base: &str) -> Result<String> {
        // TODO: --context=10000 is a hack to get full file context for the UI's
        // collapsible sections. jj doesn't have a --context=all option. Consider
        // fetching full files separately and reconstructing the diff in the UI.
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args([
                "diff",
                "--from",
                base,
                "--to",
                change_id,
                "--git",
                "--context=10000",
            ])
            .output()
            .context("Failed to run jj diff")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj diff failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    fn diff_stat(&self, change_id: &str, base: &str) -> Result<Vec<FileDiff>> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["diff", "--from", base, "--to", change_id, "--summary"])
            .output()
            .context("Failed to run jj diff --summary")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj diff --summary failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8(output.stdout)?;
        let mut files = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let status = match parts[0] {
                    "A" => FileStatus::Added,
                    "M" => FileStatus::Modified,
                    "D" => FileStatus::Deleted,
                    _ => continue,
                };
                files.push(FileDiff {
                    path: parts[1].to_string(),
                    status,
                });
            }
        }

        Ok(files)
    }

    /// Get raw git diff between two commits for a specific file
    pub fn diff_raw_between(&self, from: &str, to: &str, file: &str) -> Result<String> {
        self.diff_raw_between_ctx(from, to, file, None)
    }

    /// Get raw git diff between two commits for a specific file, with configurable context lines.
    pub fn diff_raw_between_ctx(&self, from: &str, to: &str, file: &str, context: Option<usize>) -> Result<String> {
        let ctx_flag;
        let mut args = vec!["diff", "--from", from, "--to", to, "--git"];
        if let Some(ctx) = context {
            ctx_flag = format!("--context={}", ctx);
            args.push(&ctx_flag);
        }
        args.push("--");
        args.push(file);

        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(&args)
            .output()
            .context("Failed to run jj diff")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj diff failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Show file content at a specific revision
    pub fn show_file(&self, change_id: &str, path: &str) -> Result<String> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["file", "show", "-r", change_id, path])
            .output()
            .context("Failed to run jj file show")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj file show failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Get the change_id that a bookmark points to, if it exists
    pub fn get_bookmark(&self, name: &str) -> Result<Option<String>> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["log", "--no-graph", "-r", name, "-T", "change_id"])
            .output()
            .context("Failed to run jj log for bookmark")?;

        if !output.status.success() {
            // Bookmark doesn't exist
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("doesn't exist") {
                return Ok(None);
            }
            anyhow::bail!("jj log failed: {}", stderr);
        }

        let change_id = String::from_utf8(output.stdout)?.trim().to_string();
        Ok(Some(change_id))
    }

    /// Get info about a specific change
    pub fn get_change(&self, change_id: &str) -> Result<Change> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args([
                "log",
                "--no-graph",
                "-r",
                change_id,
                "-T",
                r#"json(self) ++ "\t" ++ empty ++ "\t" ++ conflict ++ "\t" ++ self.current_working_copy() ++ "\t" ++ parents.map(|c| c.change_id()).join(",") ++ "\n""#,
            ])
            .output()
            .context("Failed to run jj log")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj log failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8(output.stdout)?;
        let line = stdout.lines().next().context("No output from jj log")?;
        let parts: Vec<&str> = line.rsplitn(5, '\t').collect();
        if parts.len() < 5 {
            anyhow::bail!("Invalid jj log output format");
        }
        let parents_str = parts[0];
        let is_wc_str = parts[1];
        let conflict_str = parts[2];
        let empty_str = parts[3];
        let json_str = parts[4];

        let jj_change: JjChange = serde_json::from_str(json_str)
            .with_context(|| format!("Failed to parse jj log output: {json_str}"))?;

        let parent_change_ids: Vec<String> = if parents_str.is_empty() {
            Vec::new()
        } else {
            parents_str.split(',').map(|s| s.to_string()).collect()
        };

        Ok(Change {
            change_id: jj_change.change_id,
            commit_id: jj_change.commit_id,
            description: jj_change.description.trim_end().to_string(),
            author: jj_change.author.email,
            timestamp: jj_change.committer.timestamp,
            empty: empty_str == "true",
            conflict: conflict_str == "true",
            is_working_copy: is_wc_str == "true",
            parent_change_ids,
        })
    }

    /// Move a bookmark to point to a specific change
    pub fn move_bookmark(&self, name: &str, change_id: &str) -> Result<()> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["bookmark", "set", name, "-r", change_id])
            .output()
            .context("Failed to run jj bookmark set")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj bookmark set failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Clone a repo via jj git clone. Returns Jj for the new clone.
    pub fn git_clone(source: &Path, dest: &Path) -> Result<Self> {
        let output = Command::new("jj")
            .args([
                "git",
                "clone",
                &source.to_string_lossy(),
                &dest.to_string_lossy(),
            ])
            .output()
            .context("Failed to run jj git clone")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj git clone failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(Self::new(dest))
    }

    pub fn git_push_bookmark(&self, bookmark: &str, allow_new: bool) -> Result<String> {
        let mut args = vec!["git", "push", "--bookmark", bookmark];
        if allow_new {
            args.push("--allow-new");
        }

        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(&args)
            .output()
            .context("Failed to run jj git push")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj git push failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    pub fn git_fetch(&self) -> Result<String> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["git", "fetch"])
            .output()
            .context("Failed to run jj git fetch")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj git fetch failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    pub fn bookmark_create(&self, name: &str, revision: &str) -> Result<()> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["bookmark", "create", name, "-r", revision])
            .output()
            .context("Failed to run jj bookmark create")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj bookmark create failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    pub fn bookmark_delete(&self, name: &str) -> Result<()> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["bookmark", "delete", name])
            .output()
            .context("Failed to run jj bookmark delete")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj bookmark delete failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    pub fn new_change(&self, message: &str) -> Result<()> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["new", "-m", message])
            .output()
            .context("Failed to run jj new")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj new failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    pub fn new_change_on(&self, revision: &str, message: &str) -> Result<()> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["new", revision, "-m", message])
            .output()
            .context("Failed to run jj new")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj new failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    pub fn describe(&self, message: &str) -> Result<()> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["describe", "-m", message])
            .output()
            .context("Failed to run jj describe")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj describe failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    pub fn rebase(&self, revision: &str, destination: &str) -> Result<String> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["rebase", "-r", revision, "-d", destination])
            .output()
            .context("Failed to run jj rebase")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj rebase failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    pub fn squash_into(&self, from: &str, into: &str) -> Result<()> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["squash", "--from", from, "--into", into])
            .output()
            .context("Failed to run jj squash")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj squash failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    pub fn abandon(&self, revision: &str) -> Result<()> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["abandon", revision])
            .output()
            .context("Failed to run jj abandon")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj abandon failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Run a revset and return the matching change_ids.
    pub fn query_change_ids(&self, revset: &str) -> Result<Vec<String>> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args([
                "log",
                "--no-graph",
                "-r",
                revset,
                "-T",
                r#"change_id ++ "\n""#,
            ])
            .output()
            .context("Failed to run jj log")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj log failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8(output.stdout)?;
        Ok(stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    /// Get the working copy change ID
    pub fn working_copy_change_id(&self) -> Result<String> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["log", "--no-graph", "-r", "@", "-T", "change_id"])
            .output()
            .context("Failed to run jj log")?;

        if !output.status.success() {
            anyhow::bail!(
                "jj log failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jj_discover() {
        // This test only works if run from within a jj repo
        if let Ok(jj) = Jj::discover() {
            assert!(jj.repo_path().exists());
        }
    }
}
