use crate::{Ec2CliError, Result};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

use super::schema::Profile;

/// Validate a profile name is safe (no path traversal)
fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Ec2CliError::ProfileInvalid(
            "Profile name cannot be empty".to_string(),
        ));
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(Ec2CliError::ProfileInvalid(format!(
            "Invalid profile name '{}': path traversal characters not allowed",
            name
        )));
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err(Ec2CliError::ProfileInvalid(format!(
            "Invalid profile name '{}': only alphanumeric, dash, and underscore allowed",
            name
        )));
    }
    Ok(())
}

pub struct ProfileLoader {
    /// Global profiles directory: ~/.config/ec2-cli/profiles/
    global_dir: Option<PathBuf>,
    /// Local profiles directory: .ec2-cli/profiles/
    local_dir: Option<PathBuf>,
}

impl ProfileLoader {
    pub fn new() -> Self {
        let global_dir = ProjectDirs::from("", "", "ec2-cli")
            .map(|dirs| dirs.config_dir().join("profiles"));

        let local_dir = std::env::current_dir()
            .ok()
            .map(|d| d.join(".ec2-cli").join("profiles"));

        Self {
            global_dir,
            local_dir,
        }
    }

    /// Load a profile by name. Order of precedence:
    /// 1. Local project profiles (.ec2-cli/profiles/)
    /// 2. Global profiles (~/.config/ec2-cli/profiles/)
    /// 3. Built-in default profile
    pub fn load(&self, name: &str) -> Result<Profile> {
        // Validate profile name to prevent path traversal attacks
        validate_profile_name(name)?;

        // Try local first
        if let Some(ref local_dir) = self.local_dir {
            if let Some(profile) = self.try_load_from_dir(local_dir, name)? {
                return Ok(profile);
            }
        }

        // Try global
        if let Some(ref global_dir) = self.global_dir {
            if let Some(profile) = self.try_load_from_dir(global_dir, name)? {
                return Ok(profile);
            }
        }

        // Fall back to built-in default
        if name == "default" {
            return Ok(Profile::default_profile());
        }

        Err(Ec2CliError::ProfileNotFound(name.to_string()))
    }

    fn try_load_from_dir(&self, dir: &Path, name: &str) -> Result<Option<Profile>> {
        // Try .json5 first, then .json
        for ext in ["json5", "json"] {
            let path = dir.join(format!("{}.{}", name, ext));
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                let profile: Profile = json5::from_str(&content).map_err(|e| {
                    Ec2CliError::ProfileInvalid(format!("Failed to parse {}: {}", path.display(), e))
                })?;
                return Ok(Some(profile));
            }
        }
        Ok(None)
    }

    /// List all available profiles
    pub fn list(&self) -> Result<Vec<ProfileInfo>> {
        let mut profiles = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        // Local profiles take precedence
        if let Some(ref local_dir) = self.local_dir {
            if local_dir.exists() {
                for entry in std::fs::read_dir(local_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if let Some(name) = extract_profile_name(&path) {
                        if seen_names.insert(name.clone()) {
                            profiles.push(ProfileInfo {
                                name,
                                source: ProfileSource::Local,
                                path: Some(path),
                            });
                        }
                    }
                }
            }
        }

        // Global profiles
        if let Some(ref global_dir) = self.global_dir {
            if global_dir.exists() {
                for entry in std::fs::read_dir(global_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if let Some(name) = extract_profile_name(&path) {
                        if seen_names.insert(name.clone()) {
                            profiles.push(ProfileInfo {
                                name,
                                source: ProfileSource::Global,
                                path: Some(path),
                            });
                        }
                    }
                }
            }
        }

        // Built-in default
        if !seen_names.contains("default") {
            profiles.push(ProfileInfo {
                name: "default".to_string(),
                source: ProfileSource::BuiltIn,
                path: None,
            });
        }

        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profiles)
    }

    /// Get the global profiles directory path
    pub fn global_dir(&self) -> Option<&PathBuf> {
        self.global_dir.as_ref()
    }

    /// Get the local profiles directory path
    pub fn local_dir(&self) -> Option<&PathBuf> {
        self.local_dir.as_ref()
    }
}

impl Default for ProfileLoader {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_profile_name(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    if ext != "json" && ext != "json5" {
        return None;
    }
    path.file_stem()?.to_str().map(String::from)
}

#[derive(Debug, Clone)]
pub struct ProfileInfo {
    pub name: String,
    pub source: ProfileSource,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    Local,
    Global,
    BuiltIn,
}

impl std::fmt::Display for ProfileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileSource::Local => write!(f, "local"),
            ProfileSource::Global => write!(f, "global"),
            ProfileSource::BuiltIn => write!(f, "built-in"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_profile() {
        let loader = ProfileLoader::new();
        let profile = loader.load("default").unwrap();
        assert_eq!(profile.name, "default");
        assert_eq!(profile.instance.instance_type, "t3.large");
        assert_eq!(profile.instance.storage.root_volume.size_gb, 30);
        profile.validate().unwrap();
    }

    #[test]
    fn test_profile_not_found() {
        let loader = ProfileLoader::new();
        let result = loader.load("nonexistent-profile-xyz");
        assert!(matches!(result, Err(Ec2CliError::ProfileNotFound(_))));
    }

    #[test]
    fn test_path_traversal_prevention() {
        let loader = ProfileLoader::new();

        // Should reject path traversal attempts
        assert!(loader.load("../../../etc/passwd").is_err());
        assert!(loader.load("..").is_err());
        assert!(loader.load("profile/subdir").is_err());
        assert!(loader.load("profile\\subdir").is_err());

        // Should accept valid profile names
        assert!(validate_profile_name("my-profile").is_ok());
        assert!(validate_profile_name("my_profile").is_ok());
        assert!(validate_profile_name("MyProfile123").is_ok());
    }
}
