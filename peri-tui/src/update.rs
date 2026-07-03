//! Update mechanism: downloads and runs the remote install script.
//!
//! On Unix: curl install.sh | bash
//! On Windows: irm install.ps1 | iex
//!
//! Delegates all update logic (download, checksum, extract, symlink)
//! to the remote install scripts.

use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::{
    io::{AsyncReadExt, BufReader},
    process::Command,
};

const SCRIPT_URL_SH: &str =
    "https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh";
const SCRIPT_URL_PS1: &str =
    "https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.ps1";

/// Run the update flow. Returns Ok(new_tag) on success.
///
/// Streams the remote install script's stdout/stderr to the terminal.
pub async fn run_update() -> Result<String> {
    println!("Peri update");

    if cfg!(target_os = "windows") {
        run_update_windows().await
    } else {
        run_update_unix().await
    }
}

async fn run_update_unix() -> Result<String> {
    println!("  Running remote install script...");

    let mut child = Command::new("bash")
        .arg("-c")
        .arg(format!("curl -fsSL {SCRIPT_URL_SH} | bash"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn update process. Is bash/curl available?")?;

    stream_output(&mut child).await?;
    read_installed_version()
}

async fn run_update_windows() -> Result<String> {
    println!("  Running remote install script...");

    let ps_command = format!("irm {SCRIPT_URL_PS1} | iex");

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps_command,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn update process. Is PowerShell available?")?;

    stream_output(&mut child).await?;
    read_installed_version()
}

/// Read all output from a child process, handling non-UTF-8 bytes gracefully.
/// Uses `String::from_utf8_lossy` to avoid crashing when the child process
/// emits bytes that are not valid UTF-8 (e.g., binary data, locale-specific
/// characters, or ANSI escape sequences with corrupted bytes).
async fn stream_output(child: &mut tokio::process::Child) -> Result<()> {
    // Take stdout and stderr handles
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Read stdout and stderr into buffers, replacing invalid UTF-8 on the fly
    if let Some(mut reader) = stdout.map(BufReader::new) {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await?;
        let text = String::from_utf8_lossy(&buf);
        for line in text.lines() {
            println!("{line}");
        }
    }

    if let Some(mut reader) = stderr.map(BufReader::new) {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await?;
        let text = String::from_utf8_lossy(&buf);
        for line in text.lines() {
            eprintln!("{line}");
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        anyhow::bail!("Update script exited with status {}", status);
    }

    Ok(())
}

fn read_installed_version() -> Result<String> {
    let version_file = dirs_next::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".peri")
        .join("current-version.txt");
    let tag = std::fs::read_to_string(&version_file)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(tag)
}
