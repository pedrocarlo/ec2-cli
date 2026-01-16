use std::process::{Command, Stdio};

use crate::{Ec2CliError, Result};

/// Push to a remote via git subprocess
pub fn git_push(
    remote: &str,
    branch: Option<&str>,
    set_upstream: bool,
    ssh_command: Option<&str>,
) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("push");

    if set_upstream {
        cmd.arg("--set-upstream");
    }

    cmd.arg(remote);

    if let Some(b) = branch {
        cmd.arg(b);
    }

    if let Some(ssh_cmd) = ssh_command {
        cmd.env("GIT_SSH_COMMAND", ssh_cmd);
    }

    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

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
pub fn git_pull(remote: &str, branch: Option<&str>, ssh_command: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("pull").arg(remote);

    if let Some(b) = branch {
        cmd.arg(b);
    }

    if let Some(ssh_cmd) = ssh_command {
        cmd.env("GIT_SSH_COMMAND", ssh_cmd);
    }

    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    let status = cmd.status().map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::Git(format!(
            "git pull failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
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
