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

    /// List recent changes
    pub fn log(&self, limit: usize) -> Result<Vec<Change>> {
        // Use json(self) for proper escaping of description, append empty flag with tab separator
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args([
                "log",
                "--no-graph",
                "-r",
                &format!("ancestors(@, {limit})"),
                "-T",
                r#"json(self) ++ "\t" ++ empty ++ "\n""#,
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

            // Parse "json\tempty" format
            // TODO: jj's json(self) doesn't include `empty`, so we append it separately.
            // Would be cleaner if jj supported including it in the JSON output.
            let Some((json_str, empty_str)) = line.rsplit_once('\t') else {
                continue;
            };

            let jj_change: JjChange = serde_json::from_str(json_str)
                .with_context(|| format!("Failed to parse jj log output: {json_str}"))?;

            // Skip the root commit (all z's) - it has no parent and can't be diffed
            if jj_change.change_id.chars().all(|c| c == 'z') {
                continue;
            }

            changes.push(Change {
                change_id: jj_change.change_id,
                commit_id: jj_change.commit_id,
                description: jj_change.description.trim_end().to_string(),
                author: jj_change.author.email,
                timestamp: jj_change.committer.timestamp,
                empty: empty_str == "true",
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
                r#"json(self) ++ "\t" ++ empty ++ "\n""#,
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
        let (json_str, empty_str) = line
            .rsplit_once('\t')
            .context("Invalid jj log output format")?;

        let jj_change: JjChange = serde_json::from_str(json_str)
            .with_context(|| format!("Failed to parse jj log output: {json_str}"))?;

        Ok(Change {
            change_id: jj_change.change_id,
            commit_id: jj_change.commit_id,
            description: jj_change.description.trim_end().to_string(),
            author: jj_change.author.email,
            timestamp: jj_change.committer.timestamp,
            empty: empty_str == "true",
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
