use anyhow::{Context, Result};
use serde::Serialize;
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
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args([
                "log",
                "--no-graph",
                "-r",
                &format!("ancestors(@, {limit})"),
                "-T",
                r#"change_id ++ "\t" ++ commit_id ++ "\t" ++ description.first_line() ++ "\t" ++ author.email() ++ "\t" ++ committer.timestamp() ++ "\t" ++ empty ++ "\n""#,
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
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 6 {
                // Skip the root commit (all z's) - it has no parent and can't be diffed
                if parts[0].chars().all(|c| c == 'z') {
                    continue;
                }
                changes.push(Change {
                    change_id: parts[0].to_string(),
                    commit_id: parts[1].to_string(),
                    description: parts[2].to_string(),
                    author: parts[3].to_string(),
                    timestamp: parts[4].to_string(),
                    empty: parts[5] == "true",
                });
            }
        }

        Ok(changes)
    }

    /// Get the commit_id for a specific change
    pub fn get_commit_id(&self, change_id: &str) -> Result<String> {
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["log", "--no-graph", "-r", change_id, "-T", "commit_id"])
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
        let output = Command::new("jj")
            .current_dir(&self.repo_path)
            .args(["diff", "--from", base, "--to", change_id, "--git"])
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
