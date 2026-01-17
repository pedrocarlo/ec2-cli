use crate::{Ec2CliError, Result};
use std::process::{Command, Stdio};

/// Options for push operations
#[derive(Debug, Default)]
pub struct PushOptions<'a> {
    /// Branch/bookmark to push (None = current)
    pub branch: Option<&'a str>,
    /// Set upstream tracking
    pub set_upstream: bool,
    /// Custom SSH command (e.g., for SSM)
    pub ssh_command: Option<&'a str>,
}

/// Options for pull/fetch operations
#[derive(Debug, Default)]
pub struct PullOptions<'a> {
    /// Branch to pull (None = current or all)
    pub branch: Option<&'a str>,
    /// Custom SSH command (e.g., for SSM)
    pub ssh_command: Option<&'a str>,
}

/// Trait for version control system operations
pub trait Vcs {
    /// Returns the VCS type identifier
    fn vcs_type(&self) -> VcsType;

    /// Push to a remote
    fn push(&self, remote: &str, options: PushOptions) -> Result<()>;

    /// Pull/fetch from a remote
    fn pull(&self, remote: &str, options: PullOptions) -> Result<()>;

    /// List all configured remotes
    fn list_remotes(&self) -> Result<Vec<String>>;

    /// Add a new remote
    fn add_remote(&self, name: &str, url: &str) -> Result<()>;

    /// Remove a remote
    fn remove_remote(&self, name: &str) -> Result<()>;

    /// Get the current branch/bookmark name
    fn current_branch(&self) -> Result<Option<String>>;

    /// Ensure a remote exists, adding it if necessary
    fn ensure_remote(&self, name: &str, url: &str) -> Result<bool> {
        let remotes = self.list_remotes()?;
        if remotes.contains(&name.to_string()) {
            Ok(false)
        } else {
            self.add_remote(name, url)?;
            Ok(true)
        }
    }
}

/// The version control system in use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsType {
    Git,
    Jj,
}

impl std::fmt::Display for VcsType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VcsType::Git => write!(f, "git"),
            VcsType::Jj => write!(f, "jj"),
        }
    }
}

/// Detect which VCS is in use in the current directory and return the appropriate implementation
pub fn detect_vcs() -> Option<Box<dyn Vcs>> {
    if Jj::is_repo() {
        Some(Box::new(Jj))
    } else if Git::is_repo() {
        Some(Box::new(Git))
    } else {
        None
    }
}

// =============================================================================
// Git Implementation
// =============================================================================

/// Git VCS implementation
#[derive(Debug, Clone, Copy)]
pub struct Git;

impl Git {
    /// Check if current directory is a git repository
    pub fn is_repo() -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

impl Vcs for Git {
    fn vcs_type(&self) -> VcsType {
        VcsType::Git
    }

    fn push(&self, remote: &str, options: PushOptions) -> Result<()> {
        let mut cmd = Command::new("git");
        cmd.arg("push");

        if options.set_upstream {
            cmd.arg("-u");
        }

        cmd.arg(remote);

        // Use explicit refspec format to avoid "no upstream branch" errors
        // when push.default=simple is set
        if let Some(b) = options.branch {
            cmd.arg(format!("{}:{}", b, b));
        }

        if let Some(ssh_cmd) = options.ssh_command {
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

    fn pull(&self, remote: &str, options: PullOptions) -> Result<()> {
        let mut cmd = Command::new("git");
        cmd.arg("pull").arg(remote);

        if let Some(b) = options.branch {
            cmd.arg(b);
        }

        if let Some(ssh_cmd) = options.ssh_command {
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

    fn list_remotes(&self) -> Result<Vec<String>> {
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

    fn add_remote(&self, name: &str, url: &str) -> Result<()> {
        let status = Command::new("git")
            .args(["remote", "add", name, url])
            .status()
            .map_err(|e| Ec2CliError::Git(e.to_string()))?;

        if !status.success() {
            return Err(Ec2CliError::Git(format!("Failed to add remote '{}'", name)));
        }

        Ok(())
    }

    fn remove_remote(&self, name: &str) -> Result<()> {
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

    fn current_branch(&self) -> Result<Option<String>> {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .map_err(|e| Ec2CliError::Git(e.to_string()))?;

        if !output.status.success() {
            return Err(Ec2CliError::Git("Failed to get current branch".to_string()));
        }

        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() || branch == "HEAD" {
            Ok(None)
        } else {
            Ok(Some(branch))
        }
    }
}

// =============================================================================
// JJ (Jujutsu) Implementation
// =============================================================================

/// Jujutsu (jj) VCS implementation
#[derive(Debug, Clone, Copy)]
pub struct Jj;

impl Jj {
    /// Check if current directory is a jj repository
    pub fn is_repo() -> bool {
        Command::new("jj")
            .args(["root", "--ignore-working-copy"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

impl Vcs for Jj {
    fn vcs_type(&self) -> VcsType {
        VcsType::Jj
    }

    fn push(&self, remote: &str, options: PushOptions) -> Result<()> {
        let mut cmd = Command::new("jj");
        cmd.args([
            "git",
            "push",
            "--ignore-working-copy",
            "--allow-new",
            "--remote",
            remote,
        ]);

        if let Some(b) = options.branch {
            cmd.args(["--bookmark", b]);
        }

        if let Some(ssh_cmd) = options.ssh_command {
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

    fn pull(&self, remote: &str, options: PullOptions) -> Result<()> {
        // JJ uses fetch instead of pull (it auto-rebases)
        // Note: branch parameter is ignored for JJ fetch as it fetches all refs
        if options.branch.is_some() {
            eprintln!("Note: JJ fetches all refs from remote, branch filter is not applied");
        }

        let mut cmd = Command::new("jj");
        cmd.args(["git", "fetch", "--ignore-working-copy", "--remote", remote]);

        if let Some(ssh_cmd) = options.ssh_command {
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

    fn list_remotes(&self) -> Result<Vec<String>> {
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

    fn add_remote(&self, name: &str, url: &str) -> Result<()> {
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

    fn remove_remote(&self, name: &str) -> Result<()> {
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

    fn current_branch(&self) -> Result<Option<String>> {
        // Get bookmarks pointing to the current working copy commit's parent
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
}
