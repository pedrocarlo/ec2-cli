use std::path::Path;

use crate::{Ec2CliError, Result};

/// Add a git remote for an EC2 instance
pub fn add_remote(
    repo_path: &Path,
    remote_name: &str,
    instance_id: &str,
    project_name: &str,
) -> Result<()> {
    let repo = git2::Repository::open(repo_path).map_err(|_| Ec2CliError::NotGitRepo)?;

    // Check if remote already exists
    if repo.find_remote(remote_name).is_ok() {
        return Err(Ec2CliError::GitRemoteExists(remote_name.to_string()));
    }

    // Build remote URL for SSH via SSM
    let remote_url = format!(
        "ec2-user@{}:/home/ec2-user/repos/{}.git",
        instance_id, project_name
    );

    repo.remote(remote_name, &remote_url)
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    Ok(())
}

/// Remove a git remote
pub fn remove_remote(repo_path: &Path, remote_name: &str) -> Result<()> {
    let repo = git2::Repository::open(repo_path).map_err(|_| Ec2CliError::NotGitRepo)?;

    repo.remote_delete(remote_name)
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    Ok(())
}

/// Get the current project name from the repository
pub fn get_project_name(repo_path: &Path) -> Result<String> {
    // Use the directory name as project name
    repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .ok_or_else(|| Ec2CliError::InvalidPath("Cannot determine project name".to_string()))
}

/// Check if a path is a git repository
pub fn is_git_repo(path: &Path) -> bool {
    git2::Repository::open(path).is_ok()
}

/// Get the remote URL for an instance
pub fn get_remote_url(instance_id: &str, project_name: &str) -> String {
    format!(
        "ec2-user@{}:/home/ec2-user/repos/{}.git",
        instance_id, project_name
    )
}

/// Check if SSH config has the required SSM proxy configuration
pub fn check_ssh_config() -> Result<SshConfigStatus> {
    let home = std::env::var("HOME").map_err(|_| {
        Ec2CliError::SshConfig("Cannot determine home directory".to_string())
    })?;

    let ssh_config_path = std::path::Path::new(&home).join(".ssh").join("config");

    if !ssh_config_path.exists() {
        return Ok(SshConfigStatus::Missing);
    }

    let content = std::fs::read_to_string(&ssh_config_path)?;

    // Check for SSM proxy configuration
    // Looking for patterns like "Host i-*" or "Host mi-*" with ProxyCommand
    let has_instance_host = content.contains("Host i-*") || content.contains("Host mi-*");
    let has_proxy_command = content.contains("ProxyCommand") && content.contains("ssm");

    if has_instance_host && has_proxy_command {
        Ok(SshConfigStatus::Configured)
    } else {
        Ok(SshConfigStatus::NeedsConfiguration)
    }
}

/// Generate the SSH config block for SSM
pub fn generate_ssh_config_block() -> String {
    r#"# EC2 SSH via SSM Session Manager
Host i-* mi-*
    User ec2-user
    ProxyCommand sh -c "aws ssm start-session --target %h --document-name AWS-StartSSHSession --parameters 'portNumber=%p'"
"#
    .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshConfigStatus {
    Configured,
    NeedsConfiguration,
    Missing,
}

impl std::fmt::Display for SshConfigStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshConfigStatus::Configured => write!(f, "configured"),
            SshConfigStatus::NeedsConfiguration => write!(f, "needs configuration"),
            SshConfigStatus::Missing => write!(f, "missing"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_get_remote_url() {
        let url = get_remote_url("i-123456", "my-project");
        assert_eq!(url, "ec2-user@i-123456:/home/ec2-user/repos/my-project.git");
    }

    #[test]
    fn test_generate_ssh_config() {
        let config = generate_ssh_config_block();
        assert!(config.contains("Host i-*"));
        assert!(config.contains("ProxyCommand"));
        assert!(config.contains("ssm"));
    }
}
