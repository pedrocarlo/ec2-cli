use std::process::{Command, Stdio};

use crate::{Ec2CliError, Result};

/// The version control system in use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsType {
    Git,
    JJ,
}

impl std::fmt::Display for VcsType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VcsType::Git => write!(f, "git"),
            VcsType::JJ => write!(f, "jj"),
        }
    }
}

/// Detect which VCS is in use in the current directory.
/// Returns JJ if a .jj directory exists, otherwise Git if a git repo is detected.
pub fn detect_vcs() -> Option<VcsType> {
    if is_jj_repo() {
        Some(VcsType::JJ)
    } else if is_git_repo() {
        Some(VcsType::Git)
    } else {
        None
    }
}

/// Check if we're in a JJ (Jujutsu) repository
pub fn is_jj_repo() -> bool {
    // Use jj root to check if we're in a jj repo
    // --ignore-working-copy avoids unnecessary working copy checks
    Command::new("jj")
        .args(["root", "--ignore-working-copy"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Push to a remote via git subprocess
///
/// Uses explicit refspec format (`branch:branch`) to bypass `push.default=simple`
/// upstream check, which would otherwise fail when the local branch has no
/// tracking branch configured.
pub fn git_push(
    remote: &str,
    branch: Option<&str>,
    set_upstream: bool,
    ssh_command: Option<&str>,
) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("push");

    if set_upstream {
        cmd.arg("-u");
    }

    cmd.arg(remote);

    // Use explicit refspec format to avoid "no upstream branch" errors
    // when push.default=simple is set
    if let Some(b) = branch {
        cmd.arg(format!("{}:{}", b, b));
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
        return Err(Ec2CliError::Git(format!("Failed to add remote '{}'", name)));
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

// =============================================================================
// JJ (Jujutsu) Operations
// =============================================================================

/// Push to a remote via jj git push subprocess
pub fn jj_push(remote: &str, bookmark: Option<&str>, ssh_command: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("jj");
    cmd.args([
        "git",
        "push",
        "--ignore-working-copy",
        "--allow-new",
        "--remote",
        remote,
    ]);

    if let Some(b) = bookmark {
        cmd.args(["--bookmark", b]);
    }

    if let Some(ssh_cmd) = ssh_command {
        cmd.env("GIT_SSH_COMMAND", ssh_cmd);
    }

    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    let status = cmd.status().map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::Git(format!(
            "jj git push failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Fetch from a remote via jj git fetch subprocess
pub fn jj_fetch(remote: &str, ssh_command: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("jj");
    cmd.args(["git", "fetch", "--ignore-working-copy", "--remote", remote]);

    if let Some(ssh_cmd) = ssh_command {
        cmd.env("GIT_SSH_COMMAND", ssh_cmd);
    }

    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    let status = cmd.status().map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::Git(format!(
            "jj git fetch failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Get list of remotes via jj
pub fn jj_list_remotes() -> Result<Vec<String>> {
    let output = Command::new("jj")
        .args(["git", "remote", "list", "--ignore-working-copy"])
        .output()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !output.status.success() {
        return Err(Ec2CliError::Git("Failed to list jj remotes".to_string()));
    }

    // jj git remote list outputs "name url" per line
    let remotes = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(String::from)
        .collect();

    Ok(remotes)
}

/// Add a remote using jj git remote add
pub fn jj_add_remote(name: &str, url: &str) -> Result<()> {
    let status = Command::new("jj")
        .args(["git", "remote", "add", "--ignore-working-copy", name, url])
        .status()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::Git(format!(
            "Failed to add jj remote '{}'",
            name
        )));
    }

    Ok(())
}

/// Remove a remote using jj git remote remove
pub fn jj_remove_remote(name: &str) -> Result<()> {
    let status = Command::new("jj")
        .args(["git", "remote", "remove", "--ignore-working-copy", name])
        .status()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::Git(format!(
            "Failed to remove jj remote '{}'",
            name
        )));
    }

    Ok(())
}

/// Get the current bookmark name in JJ (similar to git branch)
pub fn jj_get_current_bookmark() -> Result<Option<String>> {
    // Get bookmarks pointing to the current working copy commit
    let output = Command::new("jj")
        .args([
            "log",
            "--ignore-working-copy",
            "-r",
            "@-",
            "--no-graph",
            "-T",
            "bookmarks",
        ])
        .output()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !output.status.success() {
        return Err(Ec2CliError::Git(
            "Failed to get current jj bookmark".to_string(),
        ));
    }

    let bookmarks_str = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // bookmarks template returns space-separated list, take the first one
    // Format can be "main" or "main@origin" etc, strip the @remote suffix
    let bookmark = bookmarks_str
        .split_whitespace()
        .next()
        .map(|b| b.split('@').next().unwrap_or(b))
        .filter(|b| !b.is_empty())
        .map(String::from);

    Ok(bookmark)
}
