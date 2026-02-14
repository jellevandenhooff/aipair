use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;

/// Ensure a tmux session named `aipair-{name}` exists.
/// If it doesn't, create one with the given working directory.
pub fn ensure_tmux_session(name: &str, working_dir: &Path) -> Result<()> {
    let tmux_name = format!("aipair-{name}");

    // Check if session already exists
    let output = Command::new("tmux")
        .args(["has-session", "-t", &tmux_name])
        .output()
        .context("Failed to run tmux — is it installed?")?;

    if output.status.success() {
        return Ok(());
    }

    // Create new tmux session (detached)
    let output = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            &tmux_name,
            "-c",
            &working_dir.to_string_lossy(),
        ])
        .output()
        .context("Failed to create tmux session")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux new-session failed: {stderr}");
    }

    Ok(())
}

/// Spawn a PTY running `tmux attach-session -t aipair-{name}`.
/// Returns (reader, writer, master) where master can be used for resize.
pub fn spawn_terminal(
    name: &str,
    cols: u16,
    rows: u16,
) -> Result<(Box<dyn Read + Send>, Box<dyn Write + Send>, Box<dyn MasterPty + Send>)> {
    let tmux_name = format!("aipair-{name}");

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("Failed to open PTY")?;

    let mut cmd = CommandBuilder::new("tmux");
    cmd.args(["attach-session", "-t", &tmux_name]);

    pair.slave
        .spawn_command(cmd)
        .context("Failed to spawn tmux attach")?;

    let reader = pair
        .master
        .try_clone_reader()
        .context("Failed to clone PTY reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("Failed to take PTY writer")?;

    Ok((reader, writer, pair.master))
}
