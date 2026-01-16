use crate::profile::Profile;
use crate::{Ec2CliError, Result};

/// Characters that are dangerous in shell contexts
const SHELL_METACHARACTERS: &[char] = &[
    ';', '&', '|', '$', '`', '(', ')', '{', '}', '[', ']', '<', '>', '\'', '"', '\\', '\n', '\r',
    '!', '#', '*', '?', '~',
];

/// Validate a string is safe to use in shell commands.
/// Rejects strings containing shell metacharacters that could enable command injection.
fn validate_shell_safe(s: &str, context: &str) -> Result<()> {
    if s.chars().any(|c| SHELL_METACHARACTERS.contains(&c)) {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Invalid characters in {}: '{}'. Shell metacharacters are not allowed.",
            context, s
        )));
    }
    if s.is_empty() {
        return Err(Ec2CliError::ProfileValidation(format!(
            "{} cannot be empty",
            context
        )));
    }
    Ok(())
}

/// Validate an environment variable key (more restrictive - alphanumeric and underscore only)
fn validate_env_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(Ec2CliError::ProfileValidation(
            "Environment variable key cannot be empty".to_string(),
        ));
    }
    if !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Invalid environment variable key: '{}'. Only alphanumeric and underscore allowed.",
            key
        )));
    }
    if key.chars().next().map(|c| c.is_numeric()).unwrap_or(false) {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Environment variable key '{}' cannot start with a number",
            key
        )));
    }
    Ok(())
}

/// Validate a project name is safe to use in paths and shell commands
pub fn validate_project_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Ec2CliError::ProfileValidation(
            "Project name cannot be empty".to_string(),
        ));
    }
    if name.len() > 64 {
        return Err(Ec2CliError::ProfileValidation(
            "Project name cannot exceed 64 characters".to_string(),
        ));
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Invalid project name: '{}'. Only alphanumeric, dash, underscore, and dot allowed.",
            name
        )));
    }
    if name.starts_with('.') || name.starts_with('-') {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Project name '{}' cannot start with a dot or dash",
            name
        )));
    }
    Ok(())
}

/// Validate a Unix username is safe to use in shell commands
fn validate_username(username: &str) -> Result<()> {
    if username.is_empty() {
        return Err(Ec2CliError::ProfileValidation(
            "Username cannot be empty".to_string(),
        ));
    }
    // Unix usernames: alphanumeric, underscore, dash, must start with letter or underscore
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Invalid username: '{}'. Only alphanumeric, underscore, and dash allowed.",
            username
        )));
    }
    if username.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Username '{}' cannot start with a digit",
            username
        )));
    }
    Ok(())
}

/// Generate cloud-init user data script from profile
pub fn generate_user_data(profile: &Profile, project_name: Option<&str>, username: &str) -> Result<String> {
    // Validate username before using in shell commands
    validate_username(username)?;

    let mut script = String::from("#!/bin/bash\nset -ex\n\n");

    // Log to file for debugging
    script.push_str("exec > >(tee /var/log/ec2-cli-init.log) 2>&1\n\n");

    // Wait for cloud-init to complete basic setup
    script.push_str("echo 'Waiting for cloud-init...'\n");
    script.push_str("cloud-init status --wait || true\n\n");

    // Ensure SSM agent is running (pre-installed on Ubuntu 18.04+ AMIs)
    // Handle both snap-based (Ubuntu 18.04+) and deb-based (older/custom AMIs) installations
    script.push_str("echo 'Ensuring SSM agent is running...'\n");
    script.push_str("if snap list amazon-ssm-agent 2>/dev/null; then\n");
    script.push_str("    snap start amazon-ssm-agent 2>/dev/null || true\n");
    script.push_str("    systemctl enable snap.amazon-ssm-agent.amazon-ssm-agent.service 2>/dev/null || true\n");
    script.push_str("    systemctl start snap.amazon-ssm-agent.amazon-ssm-agent.service 2>/dev/null || true\n");
    script.push_str("else\n");
    script.push_str("    # Fallback to deb-based agent\n");
    script.push_str("    systemctl enable amazon-ssm-agent 2>/dev/null || true\n");
    script.push_str("    systemctl start amazon-ssm-agent 2>/dev/null || true\n");
    script.push_str("fi\n\n");

    // Validate and install system packages (Ubuntu/apt-get only)
    script.push_str("echo 'Installing system packages...'\n");
    script.push_str("apt-get update\n");
    if !profile.packages.system.is_empty() {
        for pkg in &profile.packages.system {
            validate_shell_safe(pkg, "system package name")?;
        }
        let packages = profile.packages.system.join(" ");
        script.push_str(&format!("apt-get install -y {}\n\n", packages));
    }

    // Install Docker
    script.push_str("echo 'Installing Docker...'\n");
    script.push_str("apt-get install -y docker.io\n");
    script.push_str("systemctl enable docker\n");
    script.push_str("systemctl start docker\n");
    script.push_str(&format!("usermod -aG docker {}\n\n", username));

    // Install Rust if enabled
    if profile.packages.rust.enabled {
        // Validate rust components
        for component in &profile.packages.rust.components {
            validate_shell_safe(component, "rust component")?;
        }

        script.push_str("echo 'Installing Rust...'\n");
        script.push_str(&format!("su - {} -c '\n", username));
        script.push_str("curl --proto \"=https\" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y");

        // Add channel if not stable (channel is already validated by profile.validate())
        if profile.packages.rust.channel != "stable" {
            script.push_str(&format!(
                " --default-toolchain {}",
                profile.packages.rust.channel
            ));
        }
        script.push_str("\n");

        // Source cargo env and install components
        script.push_str("source ~/.cargo/env\n");

        if !profile.packages.rust.components.is_empty() {
            let components = profile.packages.rust.components.join(" ");
            script.push_str(&format!("rustup component add {}\n", components));
        }
        script.push_str("'\n\n");

        // Install cargo packages
        if !profile.packages.cargo.is_empty() {
            // Validate cargo package names
            for pkg in &profile.packages.cargo {
                validate_shell_safe(pkg, "cargo package name")?;
            }
            script.push_str("echo 'Installing cargo packages...'\n");
            script.push_str(&format!("su - {} -c '\n", username));
            script.push_str("source ~/.cargo/env\n");
            for pkg in &profile.packages.cargo {
                script.push_str(&format!("cargo install {}\n", pkg));
            }
            script.push_str("'\n\n");
        }
    }

    // Set environment variables
    if !profile.environment.is_empty() {
        // Validate environment variable keys and values
        for (key, value) in &profile.environment {
            validate_env_key(key)?;
            validate_shell_safe(value, &format!("environment variable value for '{}'", key))?;
        }
        script.push_str("echo 'Setting environment variables...'\n");
        script.push_str(&format!("cat >> /home/{}/.bashrc << 'ENVEOF'\n", username));
        for (key, value) in &profile.environment {
            script.push_str(&format!("export {}=\"{}\"\n", key, value));
        }
        script.push_str("ENVEOF\n\n");
    }

    // Create directories for git repos
    script.push_str("echo 'Setting up git directories...'\n");
    script.push_str(&format!("mkdir -p /home/{}/repos\n", username));
    script.push_str(&format!("mkdir -p /home/{}/work\n", username));
    script.push_str(&format!("chown -R {}:{} /home/{}/repos /home/{}/work\n\n", username, username, username, username));

    // Set up git repo for the project if name provided
    if let Some(name) = project_name {
        // Project name is validated before calling this function
        script.push_str(&format!("echo 'Setting up git repo for {}...'\n", name));
        script.push_str(&format!(
            "su - {} -c 'git init --bare /home/{}/repos/{}.git'\n",
            username, username, name
        ));

        // Create post-receive hook
        script.push_str(&format!(
            r#"cat > /home/{}/repos/{}.git/hooks/post-receive << 'HOOKEOF'
#!/bin/bash
GIT_WORK_TREE=/home/{}/work/{} git checkout -f
HOOKEOF
"#,
            username, name, username, name
        ));
        script.push_str(&format!(
            "chmod +x /home/{}/repos/{}.git/hooks/post-receive\n",
            username, name
        ));
        script.push_str(&format!(
            "chown -R {}:{} /home/{}/repos/{}.git\n",
            username, username, username, name
        ));
        script.push_str(&format!(
            "mkdir -p /home/{}/work/{}\n",
            username, name
        ));
        script.push_str(&format!(
            "chown -R {}:{} /home/{}/work/{}\n\n",
            username, username, username, name
        ));
    }

    // Signal completion
    script.push_str("echo 'ec2-cli initialization complete!'\n");
    script.push_str(&format!("touch /home/{}/.ec2-cli-ready\n", username));

    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;

    #[test]
    fn test_generate_basic_user_data() {
        let profile = Profile::default_profile();
        let script = generate_user_data(&profile, Some("test-project"), "ubuntu").unwrap();

        assert!(script.contains("#!/bin/bash"));
        assert!(script.contains("rustup"));
        assert!(script.contains("git init --bare"));
        assert!(script.contains("test-project"));
        assert!(script.contains(".ec2-cli-ready"));
        assert!(script.contains("docker.io"));
        assert!(script.contains("usermod -aG docker ubuntu"));
    }

    #[test]
    fn test_generate_without_project() {
        let profile = Profile::default_profile();
        let script = generate_user_data(&profile, None, "ubuntu").unwrap();

        assert!(script.contains("#!/bin/bash"));
        assert!(!script.contains("git init --bare"));
        assert!(script.contains("docker.io"));
    }

    #[test]
    fn test_generate_with_ubuntu_user() {
        let profile = Profile::default_profile();
        let script = generate_user_data(&profile, Some("myproject"), "ubuntu").unwrap();

        assert!(script.contains("su - ubuntu"));
        assert!(script.contains("/home/ubuntu/"));
        assert!(!script.contains("ec2-user"));
    }

    #[test]
    fn test_validate_project_name_valid() {
        assert!(validate_project_name("my-project").is_ok());
        assert!(validate_project_name("my_project").is_ok());
        assert!(validate_project_name("MyProject123").is_ok());
        assert!(validate_project_name("project.name").is_ok());
    }

    #[test]
    fn test_validate_project_name_invalid() {
        assert!(validate_project_name("").is_err());
        assert!(validate_project_name("../etc/passwd").is_err());
        assert!(validate_project_name("project; rm -rf /").is_err());
        assert!(validate_project_name("-hidden").is_err());
        assert!(validate_project_name(".hidden").is_err());
        assert!(validate_project_name("a".repeat(65).as_str()).is_err());
    }

    #[test]
    fn test_shell_injection_in_packages() {
        let mut profile = Profile::default_profile();
        profile.packages.system = vec!["gcc; rm -rf /".to_string()];

        let result = generate_user_data(&profile, None, "ubuntu");
        assert!(result.is_err());
    }

    #[test]
    fn test_shell_injection_in_env_vars() {
        let mut profile = Profile::default_profile();
        profile.environment.insert("MALICIOUS".to_string(), "$(cat /etc/passwd)".to_string());

        let result = generate_user_data(&profile, None, "ubuntu");
        assert!(result.is_err());
    }
}
