use std::process::{Command, Stdio};

use crate::{Ec2CliError, Result};

/// Push to a remote via git subprocess
pub fn git_push(remote: &str, branch: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("push").arg(remote);

    if let Some(b) = branch {
        cmd.arg(b);
    }

    cmd.stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status().map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::Git(format!(
            "git push failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Pull from a remote via git subprocess
pub fn git_pull(remote: &str, branch: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("pull").arg(remote);

    if let Some(b) = branch {
        cmd.arg(b);
    }

    cmd.stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status().map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::Git(format!(
            "git pull failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Get the current branch name
pub fn get_current_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !output.status.success() {
        return Err(Ec2CliError::Git("Failed to get current branch".to_string()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if we're in a git repository
pub fn is_git_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get list of remotes
pub fn list_remotes() -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("remote")
        .output()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !output.status.success() {
        return Err(Ec2CliError::Git("Failed to list remotes".to_string()));
    }

    let remotes = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(String::from)
        .collect();

    Ok(remotes)
}

/// Get URL for a remote
pub fn get_remote_url(remote: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", remote])
        .output()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !output.status.success() {
        return Err(Ec2CliError::Git(format!("Remote '{}' not found", remote)));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Add a remote using git command
pub fn add_remote(name: &str, url: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["remote", "add", name, url])
        .status()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::Git(format!(
            "Failed to add remote '{}'",
            name
        )));
    }

    Ok(())
}

/// Remove a remote using git command
pub fn remove_remote(name: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["remote", "remove", name])
        .status()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::Git(format!(
            "Failed to remove remote '{}'",
            name
        )));
    }

    Ok(())
}
