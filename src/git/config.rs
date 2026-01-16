use std::process::Command;

/// Git user configuration (name and email)
#[derive(Debug, Clone, Default)]
pub struct GitUserConfig {
    pub name: Option<String>,
    pub email: Option<String>,
}

impl GitUserConfig {
    /// Returns true if at least one config value is present
    pub fn has_config(&self) -> bool {
        self.name.is_some() || self.email.is_some()
    }
}

/// Find the local user's git configuration (user.name and user.email).
/// Returns a GitUserConfig with optional values - missing config is not an error.
pub fn find_git_user_config() -> GitUserConfig {
    GitUserConfig {
        name: get_git_config_value("user.name"),
        email: get_git_config_value("user.email"),
    }
}

/// Get a single git config value by key from global config.
/// Uses --global to ensure we get the user's global setting, not a repo-specific override.
/// Returns None if git is not installed, the key is not set, or the value is empty.
fn get_git_config_value(key: &str) -> Option<String> {
    Command::new("git")
        .args(["config", "--global", "--get", key])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_git_user_config_returns_struct() {
        // This test just verifies the function runs without panicking
        // Actual values depend on the user's git config
        let config = find_git_user_config();
        // GitUserConfig should always be returned (even if empty)
        let _ = config.name;
        let _ = config.email;
    }

    #[test]
    fn test_has_config_empty() {
        let config = GitUserConfig::default();
        assert!(!config.has_config());
    }

    #[test]
    fn test_has_config_with_name() {
        let config = GitUserConfig {
            name: Some("John Doe".to_string()),
            email: None,
        };
        assert!(config.has_config());
    }

    #[test]
    fn test_has_config_with_email() {
        let config = GitUserConfig {
            name: None,
            email: Some("john@example.com".to_string()),
        };
        assert!(config.has_config());
    }

    #[test]
    fn test_has_config_with_both() {
        let config = GitUserConfig {
            name: Some("John Doe".to_string()),
            email: Some("john@example.com".to_string()),
        };
        assert!(config.has_config());
    }
}
