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
pub fn generate_user_data(
    profile: &Profile,
    project_name: Option<&str>,
    username: &str,
    ssh_public_key: Option<&str>,
) -> Result<String> {
    // Validate username before using in shell commands
    validate_username(username)?;

    let mut script = String::from("#!/bin/bash\nset -ex\n\n");

    // Log to file for debugging
    script.push_str("exec > >(tee /var/log/ec2-cli-init.log) 2>&1\n\n");

    // Add SSH public key FIRST - before any blocking operations
    // This ensures SSH access is available as soon as SSM is ready
    if let Some(key) = ssh_public_key {
        script.push_str("echo 'Configuring SSH public key...'\n");
        // Note: Home directory /home/{username} is pre-created by Ubuntu AMI
        script.push_str(&format!("mkdir -p /home/{}/.ssh\n", username));
        // SSH keys are validated in key_loader to contain only base64 chars,
        // so this quoted here-document is safe from injection
        script.push_str(&format!(
            "cat >> /home/{}/.ssh/authorized_keys << 'SSHEOF'\n",
            username
        ));
        script.push_str(key);
        script.push_str("\nSSHEOF\n");
        // Set correct permissions (critical for SSH to work)
        script.push_str(&format!("chmod 700 /home/{}/.ssh\n", username));
        script.push_str(&format!(
            "chmod 600 /home/{}/.ssh/authorized_keys\n",
            username
        ));
        script.push_str(&format!(
            "chown -R {}:{} /home/{}/.ssh\n\n",
            username, username, username
        ));
    }

    // Create git repo directories and bare repo EARLY - before cloud-init wait
    // This ensures `ec2-cli push` works as soon as SSM is ready, without waiting
    // for the full cloud-init/package installation to complete
    script.push_str("echo 'Setting up git directories...'\n");
    script.push_str(&format!("mkdir -p /home/{}/repos\n", username));
    script.push_str(&format!("mkdir -p /home/{}/work\n", username));
    script.push_str(&format!(
        "chown -R {}:{} /home/{}/repos /home/{}/work\n\n",
        username, username, username, username
    ));

    // Set up git repo for the project if name provided
    if let Some(name) = project_name {
        // Project name is validated before calling this function
        script.push_str(&format!("echo 'Setting up git repo for {}...'\n", name));
        script.push_str(&format!(
            "su - {} -c 'git init --bare /home/{}/repos/{}.git'\n",
            username, username, name
        ));

        // Create post-receive hook that checks out whatever branch is pushed
        // Handles: arbitrary branch names, skips tag pushes and branch deletions
        script.push_str(&format!(
            r#"cat > /home/{}/repos/{}.git/hooks/post-receive << 'HOOKEOF'
#!/bin/bash
while read oldrev newrev refname; do
    # Skip branch deletions (newrev is all zeros)
    if [ "$newrev" = "0000000000000000000000000000000000000000" ]; then
        continue
    fi
    # Only handle branch pushes, not tags
    case "$refname" in
        refs/heads/*)
            branch="${{refname#refs/heads/}}"
            GIT_WORK_TREE=/home/{}/work/{} git checkout -f "$branch"
            ;;
    esac
done
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
        script.push_str(&format!("mkdir -p /home/{}/work/{}\n", username, name));

        // Configure bare repo to know its worktree location
        // Set core.bare=false since we're adding a worktree to a bare repo
        script.push_str(&format!(
            "git -C /home/{}/repos/{}.git config core.bare false\n",
            username, name
        ));
        script.push_str(&format!(
            "git -C /home/{}/repos/{}.git config core.worktree /home/{}/work/{}\n",
            username, name, username, name
        ));

        // Create .git file in work directory pointing to the bare repo
        // This allows normal git commands to work in ~/work/<project>/
        script.push_str(&format!(
            "echo 'gitdir: /home/{}/repos/{}.git' > /home/{}/work/{}/.git\n",
            username, name, username, name
        ));

        script.push_str(&format!(
            "chown -R {}:{} /home/{}/work/{}\n\n",
            username, username, username, name
        ));

        // Create README in home directory with usage instructions
        script.push_str(&format!(
            r#"cat > /home/{}/README.md << 'READMEEOF'
# ec2-cli Development Instance

## Your project is at:
    cd ~/work/{}

## Git workflow

Make changes, then commit normally:
    cd ~/work/{}
    git add .
    git commit -m "your message"

Then on your local machine, pull the changes:
    ec2-cli pull

## Logs

View cloud-init logs:
    cat /var/log/ec2-cli-init.log

Check if setup is complete:
    ls ~/.ec2-cli-ready
READMEEOF
"#,
            username, name, name
        ));
        script.push_str(&format!("chown {}:{} /home/{}/README.md\n\n", username, username, username));

        // Create marker file to signal git repo is ready
        script.push_str(&format!("touch /home/{}/.ec2-cli-git-ready\n\n", username));
    }

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
        script.push('\n');

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
        let script =
            generate_user_data(&profile, Some("test-project"), "ubuntu", None).unwrap();

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
        let script = generate_user_data(&profile, None, "ubuntu", None).unwrap();

        assert!(script.contains("#!/bin/bash"));
        assert!(!script.contains("git init --bare"));
        assert!(script.contains("docker.io"));
    }

    #[test]
    fn test_generate_with_ubuntu_user() {
        let profile = Profile::default_profile();
        let script =
            generate_user_data(&profile, Some("myproject"), "ubuntu", None).unwrap();

        assert!(script.contains("su - ubuntu"));
        assert!(script.contains("/home/ubuntu/"));
        assert!(!script.contains("ec2-user"));
    }

    #[test]
    fn test_generate_with_ssh_key() {
        let profile = Profile::default_profile();
        // Use a realistic key length (at least 50 chars base64)
        let ssh_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx user@example.com";
        let script =
            generate_user_data(&profile, Some("test-project"), "ubuntu", Some(ssh_key)).unwrap();

        assert!(script.contains("mkdir -p /home/ubuntu/.ssh"));
        assert!(script.contains("authorized_keys"));
        assert!(script.contains(ssh_key));
        assert!(script.contains("chmod 700 /home/ubuntu/.ssh"));
        assert!(script.contains("chmod 600 /home/ubuntu/.ssh/authorized_keys"));
        assert!(script.contains("chown -R ubuntu:ubuntu /home/ubuntu/.ssh"));
    }

    #[test]
    fn test_generate_without_ssh_key() {
        let profile = Profile::default_profile();
        let script = generate_user_data(&profile, None, "ubuntu", None).unwrap();

        assert!(!script.contains("Configuring SSH public key"));
        assert!(!script.contains("authorized_keys"));
    }

    #[test]
    fn test_ssh_key_injected_before_cloud_init_wait() {
        // Ensure SSH key is available immediately when SSM reports ready,
        // not after cloud-init completes (which can take minutes)
        let profile = Profile::default_profile();
        let ssh_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx user@example.com";
        let script =
            generate_user_data(&profile, Some("test-project"), "ubuntu", Some(ssh_key)).unwrap();

        let ssh_config_pos = script
            .find("Configuring SSH public key")
            .expect("SSH config not found");
        let cloud_init_pos = script
            .find("Waiting for cloud-init")
            .expect("cloud-init wait not found");

        assert!(
            ssh_config_pos < cloud_init_pos,
            "SSH key setup must occur before cloud-init wait to avoid race condition"
        );
    }

    #[test]
    fn test_git_ready_marker_created_after_repo_setup() {
        let profile = Profile::default_profile();
        let script =
            generate_user_data(&profile, Some("test-project"), "ubuntu", None).unwrap();

        let repo_setup_pos = script
            .find("git init --bare")
            .expect("git init not found");
        let marker_pos = script
            .find(".ec2-cli-git-ready")
            .expect("marker not found");

        assert!(
            marker_pos > repo_setup_pos,
            "Marker file must be created after git repo setup"
        );
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

        let result = generate_user_data(&profile, None, "ubuntu", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_shell_injection_in_env_vars() {
        let mut profile = Profile::default_profile();
        profile.environment.insert("MALICIOUS".to_string(), "$(cat /etc/passwd)".to_string());

        let result = generate_user_data(&profile, None, "ubuntu", None);
        assert!(result.is_err());
    }
}
