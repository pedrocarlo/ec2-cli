use std::path::PathBuf;

use crate::{Ec2CliError, Result};

/// Standard SSH key filenames to check in ~/.ssh/
const STANDARD_KEY_NAMES: &[&str] = &["id_ed25519", "id_rsa", "id_ecdsa"];

/// Find and load the user's SSH public key.
///
/// Checks locations in this order:
/// 1. `.ec2-cli/ssh_public_key` in the current directory (project-level override)
/// 2. `~/.ssh/id_ed25519.pub` (modern default)
/// 3. `~/.ssh/id_rsa.pub` (legacy but common)
/// 4. `~/.ssh/id_ecdsa.pub` (ECDSA keys)
pub fn find_ssh_public_key() -> Result<String> {
    let mut checked_paths = Vec::new();

    // 1. Check .ec2-cli/ssh_public_key in current directory
    if let Ok(cwd) = std::env::current_dir() {
        let local_key_path = cwd.join(".ec2-cli").join("ssh_public_key");
        match try_load_key(&local_key_path) {
            Ok(key) => return Ok(key),
            Err(LoadKeyError::NotFound) => {
                checked_paths.push(local_key_path.display().to_string());
            }
            Err(LoadKeyError::ReadError(path, e)) => {
                return Err(Ec2CliError::SshKeyInvalid(format!(
                    "Cannot read SSH key from {}: {}",
                    path.display(),
                    e
                )));
            }
            Err(LoadKeyError::Invalid(msg)) => {
                return Err(Ec2CliError::SshKeyInvalid(msg));
            }
        }
    }

    // 2. Check standard SSH key locations in ~/.ssh/
    if let Some(home) = home_dir() {
        for key_name in STANDARD_KEY_NAMES {
            let path = home.join(".ssh").join(format!("{}.pub", key_name));
            match try_load_key(&path) {
                Ok(key) => return Ok(key),
                Err(LoadKeyError::NotFound) => {
                    checked_paths.push(path.display().to_string());
                }
                Err(LoadKeyError::ReadError(path, e)) => {
                    return Err(Ec2CliError::SshKeyInvalid(format!(
                        "Cannot read SSH key from {}: {}",
                        path.display(),
                        e
                    )));
                }
                Err(LoadKeyError::Invalid(msg)) => {
                    return Err(Ec2CliError::SshKeyInvalid(msg));
                }
            }
        }
    }

    Err(Ec2CliError::SshKeyNotFound(checked_paths.join(", ")))
}

/// Internal error type for key loading
enum LoadKeyError {
    NotFound,
    ReadError(PathBuf, std::io::Error),
    Invalid(String),
}

/// Try to load and validate an SSH key from a path.
/// Avoids TOCTOU by attempting to read directly instead of checking exists first.
fn try_load_key(path: &PathBuf) -> std::result::Result<String, LoadKeyError> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let key = content.trim().to_string();
            validate_ssh_key_format(&key).map_err(|e| LoadKeyError::Invalid(e.to_string()))?;
            Ok(key)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(LoadKeyError::NotFound),
        Err(e) => Err(LoadKeyError::ReadError(path.clone(), e)),
    }
}

/// Validate that a string is a valid single-line OpenSSH public key.
fn validate_ssh_key_format(key: &str) -> Result<()> {
    let key = key.trim();

    if key.is_empty() {
        return Err(Ec2CliError::SshKeyInvalid("SSH key is empty".to_string()));
    }

    // Reject multi-line keys (security: prevents authorized_keys injection)
    if key.contains('\n') || key.contains('\r') {
        return Err(Ec2CliError::SshKeyInvalid(
            "SSH key contains multiple lines. Only single-line keys are supported.".to_string(),
        ));
    }

    // Valid OpenSSH public key formats
    let valid_prefixes = ["ssh-rsa ", "ssh-ed25519 ", "ecdsa-sha2-nistp"];

    let is_valid_prefix = valid_prefixes.iter().any(|prefix| key.starts_with(prefix));
    if !is_valid_prefix {
        return Err(Ec2CliError::SshKeyInvalid(format!(
            "Invalid SSH public key format. Must start with 'ssh-rsa', 'ssh-ed25519', or 'ecdsa-sha2-nistp*'. Got: {}...",
            &key[..key.len().min(30)]
        )));
    }

    // Parse the key parts: type, base64-encoded key material, optional comment
    let parts: Vec<&str> = key.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(Ec2CliError::SshKeyInvalid(
            "SSH key appears malformed (missing key data)".to_string(),
        ));
    }

    // Validate key material is valid base64 characters
    let key_material = parts[1];
    if !key_material
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
    {
        return Err(Ec2CliError::SshKeyInvalid(
            "SSH key material contains invalid characters (expected base64)".to_string(),
        ));
    }

    // Validate minimum length (ed25519 keys have ~68 chars, RSA/ECDSA have more)
    if key_material.len() < 50 {
        return Err(Ec2CliError::SshKeyInvalid(
            "SSH key material too short (expected at least 50 characters)".to_string(),
        ));
    }

    Ok(())
}

/// Get the user's home directory.
fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_rsa_key() {
        // Realistic RSA key length (truncated for readability but >= 100 chars base64)
        let key = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDKJv9EJa0VR5n5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5 user@host";
        assert!(validate_ssh_key_format(key).is_ok());
    }

    #[test]
    fn test_validate_ed25519_key() {
        // Real ed25519 key (68 chars base64)
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx user@host";
        assert!(validate_ssh_key_format(key).is_ok());
    }

    #[test]
    fn test_validate_ecdsa_key() {
        let key = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBFxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx user@host";
        assert!(validate_ssh_key_format(key).is_ok());
    }

    #[test]
    fn test_invalid_key_format() {
        let key = "not-a-valid-key";
        assert!(validate_ssh_key_format(key).is_err());
    }

    #[test]
    fn test_empty_key() {
        assert!(validate_ssh_key_format("").is_err());
        assert!(validate_ssh_key_format("   ").is_err());
    }

    #[test]
    fn test_key_without_data() {
        let key = "ssh-rsa";
        assert!(validate_ssh_key_format(key).is_err());
    }

    #[test]
    fn test_key_too_short() {
        // Less than 50 chars base64
        let key = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDK user@host";
        assert!(validate_ssh_key_format(key).is_err());
    }

    #[test]
    fn test_multiline_key_rejected() {
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx user1\nssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDKx user2";
        assert!(validate_ssh_key_format(key).is_err());
    }

    #[test]
    fn test_key_with_embedded_newline_rejected() {
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx user@host\nexec bash";
        assert!(validate_ssh_key_format(key).is_err());
    }

    #[test]
    fn test_invalid_base64_characters() {
        let key = "ssh-rsa AAAAB3NzaC1yc2!@#$%^&*()EAAAADAQABAAABgQDKJv9EJa0VR5n5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5x5X5 user@host";
        assert!(validate_ssh_key_format(key).is_err());
    }
}
