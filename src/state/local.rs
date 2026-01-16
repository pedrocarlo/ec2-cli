use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::{Ec2CliError, Result};

/// State file structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    pub instances: HashMap<String, InstanceState>,
}

/// State for a single instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceState {
    pub instance_id: String,
    pub profile: String,
    pub region: String,
    pub created_at: DateTime<Utc>,
    /// SSH username for the instance (always "ubuntu" for Ubuntu AMIs)
    #[serde(default = "default_username")]
    pub username: String,
    /// Security group ID for cleanup on termination
    #[serde(default)]
    pub security_group_id: Option<String>,
    /// Path to the SSH private key used for this instance
    #[serde(default)]
    pub ssh_key_path: Option<String>,
}

fn default_username() -> String {
    "ubuntu".to_string()
}

impl State {
    /// Load state from file
    pub fn load() -> Result<Self> {
        let path = state_file_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)?;
        let state: State = serde_json::from_str(&content).map_err(|e| {
            Ec2CliError::StateCorrupted(format!("Failed to parse state file: {}", e))
        })?;

        Ok(state)
    }

    /// Save state to file with restricted permissions (0600)
    pub fn save(&self) -> Result<()> {
        let path = state_file_path()?;

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;

        // Write with restricted permissions (owner read/write only)
        #[cfg(unix)]
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(content.as_bytes())?;
        }

        #[cfg(not(unix))]
        {
            std::fs::write(&path, content)?;
        }

        Ok(())
    }

    /// Add or update an instance
    #[allow(clippy::too_many_arguments)]
    pub fn add_instance(
        &mut self,
        name: &str,
        instance_id: &str,
        profile: &str,
        region: &str,
        username: &str,
        security_group_id: &str,
        ssh_key_path: Option<&str>,
    ) {
        self.instances.insert(
            name.to_string(),
            InstanceState {
                instance_id: instance_id.to_string(),
                profile: profile.to_string(),
                region: region.to_string(),
                created_at: Utc::now(),
                username: username.to_string(),
                security_group_id: Some(security_group_id.to_string()),
                ssh_key_path: ssh_key_path.map(String::from),
            },
        );
    }

    /// Remove an instance
    pub fn remove_instance(&mut self, name: &str) -> Option<InstanceState> {
        self.instances.remove(name)
    }

    /// Get an instance by name
    pub fn get_instance(&self, name: &str) -> Option<&InstanceState> {
        self.instances.get(name)
    }
}

/// Get the path to the state file
fn state_file_path() -> Result<PathBuf> {
    // Use XDG state directory: ~/.local/state/ec2-cli/state.json
    let base_dir = ProjectDirs::from("", "", "ec2-cli")
        .and_then(|dirs| dirs.state_dir().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| {
            // Fallback to home directory
            std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".local")
                .join("state")
                .join("ec2-cli")
        });

    Ok(base_dir.join("state.json"))
}

/// Save an instance to state (convenience function)
pub fn save_instance(
    name: &str,
    instance_id: &str,
    profile: &str,
    region: &str,
    username: &str,
    security_group_id: &str,
    ssh_key_path: Option<&str>,
) -> Result<()> {
    let mut state = State::load()?;
    state.add_instance(
        name,
        instance_id,
        profile,
        region,
        username,
        security_group_id,
        ssh_key_path,
    );
    state.save()
}

/// Remove an instance from state (convenience function)
pub fn remove_instance(name: &str) -> Result<Option<InstanceState>> {
    let mut state = State::load()?;
    let removed = state.remove_instance(name);
    state.save()?;
    Ok(removed)
}

/// Get instance state by name (convenience function)
pub fn get_instance(name: &str) -> Result<Option<InstanceState>> {
    let state = State::load()?;
    Ok(state.get_instance(name).cloned())
}

/// List all instances (convenience function)
pub fn list_instances() -> Result<HashMap<String, InstanceState>> {
    let state = State::load()?;
    Ok(state.instances)
}

/// Get linked instance name from current directory
/// Uses atomic read to avoid TOCTOU race conditions
pub fn get_linked_instance() -> Result<Option<String>> {
    let link_file = std::env::current_dir()?.join(".ec2-cli").join("instance");

    // Check for symlink attack
    if link_file.is_symlink() {
        return Err(Ec2CliError::InvalidPath(
            "Link file cannot be a symlink".to_string(),
        ));
    }

    // Read directly without checking exists() first to avoid TOCTOU
    match std::fs::read_to_string(&link_file) {
        Ok(content) => {
            let name = content.trim().to_string();
            if name.is_empty() {
                Ok(None)
            } else {
                Ok(Some(name))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Resolve instance name - use provided name or fall back to linked instance
pub fn resolve_instance_name(name: Option<&str>) -> Result<String> {
    if let Some(n) = name {
        return Ok(n.to_string());
    }

    get_linked_instance()?.ok_or_else(|| {
        Ec2CliError::InstanceNotFound(
            "No instance name provided and no linked instance found".to_string(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_operations() {
        let mut state = State::default();

        state.add_instance(
            "test-instance",
            "i-123456",
            "default",
            "us-west-2",
            "ubuntu",
            "sg-12345678",
            Some("/home/user/.ssh/id_ed25519"),
        );
        assert!(state.get_instance("test-instance").is_some());
        assert_eq!(
            state.get_instance("test-instance").unwrap().username,
            "ubuntu"
        );
        assert_eq!(
            state
                .get_instance("test-instance")
                .unwrap()
                .security_group_id,
            Some("sg-12345678".to_string())
        );
        assert_eq!(
            state.get_instance("test-instance").unwrap().ssh_key_path,
            Some("/home/user/.ssh/id_ed25519".to_string())
        );

        let removed = state.remove_instance("test-instance");
        assert!(removed.is_some());
        assert!(state.get_instance("test-instance").is_none());
    }

    #[test]
    fn test_state_with_ubuntu_user() {
        let mut state = State::default();

        state.add_instance(
            "ubuntu-instance",
            "i-789",
            "ubuntu-profile",
            "us-east-1",
            "ubuntu",
            "sg-abc",
            None,
        );
        let instance = state.get_instance("ubuntu-instance").unwrap();
        assert_eq!(instance.username, "ubuntu");
        assert_eq!(instance.ssh_key_path, None);
    }
}
