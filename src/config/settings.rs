use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::{Ec2CliError, Result};

/// Global settings for ec2-cli
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Custom tags to apply to all AWS resources
    #[serde(default)]
    pub tags: HashMap<String, String>,

    /// AWS region override (None = use AWS default from environment/config)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// VPC ID to use (None = use default VPC)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vpc_id: Option<String>,

    /// Subnet ID to launch instances in
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subnet_id: Option<String>,
}

impl Settings {
    /// Get the path to the config file
    pub fn config_path() -> Option<PathBuf> {
        ProjectDirs::from("", "", "ec2-cli")
            .map(|dirs| dirs.config_dir().join("config.json"))
    }

    /// Load settings from the config file
    pub fn load() -> Result<Self> {
        let path = Self::config_path()
            .ok_or_else(|| Ec2CliError::Config("Cannot determine config directory".to_string()))?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)?;
        let settings: Settings = serde_json::from_str(&content).map_err(|e| {
            Ec2CliError::Config(format!("Failed to parse config file: {}", e))
        })?;

        Ok(settings)
    }

    /// Save settings to the config file with restricted permissions (0600)
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()
            .ok_or_else(|| Ec2CliError::Config("Cannot determine config directory".to_string()))?;

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

    /// Validate a tag key
    pub fn validate_tag_key(key: &str) -> Result<()> {
        if key.is_empty() {
            return Err(Ec2CliError::Config("Tag key cannot be empty".to_string()));
        }
        if key.len() > 128 {
            return Err(Ec2CliError::Config(
                "Tag key cannot exceed 128 characters".to_string(),
            ));
        }
        if key.starts_with("aws:") {
            return Err(Ec2CliError::Config(
                "Tag key cannot start with 'aws:' (reserved prefix)".to_string(),
            ));
        }
        if !key.chars().all(|c| c.is_ascii() && (' '..='~').contains(&c)) {
            return Err(Ec2CliError::Config(
                "Tag key must contain only ASCII printable characters".to_string(),
            ));
        }
        Ok(())
    }

    /// Validate a tag value
    pub fn validate_tag_value(value: &str) -> Result<()> {
        if value.len() > 256 {
            return Err(Ec2CliError::Config(
                "Tag value cannot exceed 256 characters".to_string(),
            ));
        }
        if !value.chars().all(|c| c.is_ascii() && (' '..='~').contains(&c)) {
            return Err(Ec2CliError::Config(
                "Tag value must contain only ASCII printable characters".to_string(),
            ));
        }
        Ok(())
    }

    /// Set a tag (validates key and value)
    pub fn set_tag(&mut self, key: &str, value: &str) -> Result<()> {
        Self::validate_tag_key(key)?;
        Self::validate_tag_value(value)?;
        self.tags.insert(key.to_string(), value.to_string());
        Ok(())
    }

    /// Remove a tag
    pub fn remove_tag(&mut self, key: &str) -> Option<String> {
        self.tags.remove(key)
    }

    /// Check if Username tag is configured
    pub fn has_username_tag(&self) -> bool {
        self.tags.contains_key("Username")
    }

    /// Validate AWS region format (e.g., us-east-1, eu-west-2)
    pub fn validate_region(region: &str) -> Result<()> {
        // Simple validation: regions are like "us-east-1", "eu-west-2", "ap-southeast-1"
        let parts: Vec<&str> = region.split('-').collect();
        if parts.len() < 3 {
            return Err(Ec2CliError::Config(format!(
                "Invalid AWS region format: '{}'. Expected format like 'us-east-1'",
                region
            )));
        }
        // Check that the last part is a number
        if parts.last().map(|p| p.parse::<u32>().is_err()).unwrap_or(true) {
            return Err(Ec2CliError::Config(format!(
                "Invalid AWS region format: '{}'. Expected format like 'us-east-1'",
                region
            )));
        }
        Ok(())
    }

    /// Validate VPC ID format (vpc-xxxxxxxx or vpc-xxxxxxxxxxxxxxxxx)
    pub fn validate_vpc_id(vpc_id: &str) -> Result<()> {
        if !vpc_id.starts_with("vpc-") {
            return Err(Ec2CliError::Config(format!(
                "Invalid VPC ID format: '{}'. Must start with 'vpc-'",
                vpc_id
            )));
        }
        let suffix = &vpc_id[4..];
        if suffix.len() < 8 || !suffix.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Ec2CliError::Config(format!(
                "Invalid VPC ID format: '{}'. Expected format like 'vpc-12345678'",
                vpc_id
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_tag_key_valid() {
        assert!(Settings::validate_tag_key("Username").is_ok());
        assert!(Settings::validate_tag_key("Project").is_ok());
        assert!(Settings::validate_tag_key("my-tag-123").is_ok());
    }

    #[test]
    fn test_validate_tag_key_invalid() {
        assert!(Settings::validate_tag_key("").is_err());
        assert!(Settings::validate_tag_key("aws:reserved").is_err());
        assert!(Settings::validate_tag_key(&"a".repeat(129)).is_err());
        assert!(Settings::validate_tag_key("tag\nkey").is_err());
    }

    #[test]
    fn test_validate_tag_value_valid() {
        assert!(Settings::validate_tag_value("myvalue").is_ok());
        assert!(Settings::validate_tag_value("").is_ok()); // Empty is allowed
        assert!(Settings::validate_tag_value("value with spaces").is_ok());
    }

    #[test]
    fn test_validate_tag_value_invalid() {
        assert!(Settings::validate_tag_value(&"a".repeat(257)).is_err());
        assert!(Settings::validate_tag_value("value\nwith\nnewlines").is_err());
    }

    #[test]
    fn test_set_tag() {
        let mut settings = Settings::default();
        assert!(settings.set_tag("Username", "testuser").is_ok());
        assert_eq!(settings.tags.get("Username"), Some(&"testuser".to_string()));
    }

    #[test]
    fn test_remove_tag() {
        let mut settings = Settings::default();
        settings.tags.insert("Username".to_string(), "testuser".to_string());
        let removed = settings.remove_tag("Username");
        assert_eq!(removed, Some("testuser".to_string()));
        assert!(!settings.tags.contains_key("Username"));
    }

    #[test]
    fn test_has_username_tag() {
        let mut settings = Settings::default();
        assert!(!settings.has_username_tag());
        settings.tags.insert("Username".to_string(), "testuser".to_string());
        assert!(settings.has_username_tag());
    }
}
