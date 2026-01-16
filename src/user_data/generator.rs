use crate::git::GitUserConfig;
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
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
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
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Invalid username: '{}'. Only alphanumeric, underscore, and dash allowed.",
            username
        )));
    }
    if username
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Username '{}' cannot start with a digit",
            username
        )));
    }
    Ok(())
}

/// Characters that are dangerous in git config values.
/// More permissive than SHELL_METACHARACTERS - allows spaces, @, ., +, - for names/emails.
const GIT_CONFIG_DANGEROUS_CHARS: &[char] = &[
    ';', '&', '|', '$', '`', '(', ')', '{', '}', '[', ']', '<', '>', '\'', '"', '\\', '\n', '\r',
    '!', '#', '*', '?', '~',
];

/// Maximum length for git config values (prevents user-data size issues)
const GIT_CONFIG_MAX_LENGTH: usize = 256;

/// Validate a git config value is safe to use in shell commands.
/// Allows spaces, @, ., +, - which are needed for names and email addresses.
/// Blocks shell metacharacters that could enable command injection.
fn validate_git_config_value(value: &str, context: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Ec2CliError::ProfileValidation(format!(
            "{} cannot be empty",
            context
        )));
    }
    if value.len() > GIT_CONFIG_MAX_LENGTH {
        return Err(Ec2CliError::ProfileValidation(format!(
            "{} exceeds maximum length of {} characters",
            context, GIT_CONFIG_MAX_LENGTH
        )));
    }
    if value
        .chars()
        .any(|c| GIT_CONFIG_DANGEROUS_CHARS.contains(&c))
    {
        return Err(Ec2CliError::ProfileValidation(format!(
            "Invalid characters in {}: '{}'. Shell metacharacters are not allowed.",
            context, value
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
    git_user_config: Option<&GitUserConfig>,
) -> Result<String> {
    // Validate username before using in shell commands
    validate_username(username)?;

    // Validate git config values if provided
    if let Some(config) = git_user_config {
        if let Some(ref name) = config.name {
            validate_git_config_value(name, "git user.name")?;
        }
        if let Some(ref email) = config.email {
            validate_git_config_value(email, "git user.email")?;
        }
    }

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

    // Configure git user identity if provided
    if let Some(config) = git_user_config {
        if config.has_config() {
            script.push_str("echo 'Configuring git user identity...'\n");
            if let Some(ref name) = config.name {
                script.push_str(&format!(
                    "su - {} -c 'git config --global user.name \"{}\"'\n",
                    username, name
                ));
            }
            if let Some(ref email) = config.email {
                script.push_str(&format!(
                    "su - {} -c 'git config --global user.email \"{}\"'\n",
                    username, email
                ));
            }
            script.push('\n');
        }
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

    // Create docker group early and add user
    // This ensures docker group membership is active when user connects via SSM,
    // even if they connect before Docker installation completes
    script.push_str("echo 'Setting up docker group...'\n");
    script.push_str("groupadd -f docker\n"); // -f: don't fail if group exists
    script.push_str(&format!("usermod -aG docker {}\n\n", username));

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
        // Note: Use --git-dir instead of -C because -C looks for .git subdirectory
        // which doesn't exist in bare repos (the path itself IS the git directory)
        script.push_str(&format!(
            "git --git-dir=/home/{}/repos/{}.git config core.bare false\n",
            username, name
        ));
        script.push_str(&format!(
            "git --git-dir=/home/{}/repos/{}.git config core.worktree /home/{}/work/{}\n",
            username, name, username, name
        ));
        // Allow pushes to checked-out branch and auto-update working tree
        script.push_str(&format!(
            "git --git-dir=/home/{}/repos/{}.git config receive.denyCurrentBranch updateInstead\n",
            username, name
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

        // Configure MOTD to show formatted instance info on login
        script.push_str("echo 'Configuring login message...'\n");
        // Disable default Ubuntu MOTD components
        script.push_str("chmod -x /etc/update-motd.d/* 2>/dev/null || true\n");
        // Create custom MOTD script with box-formatted output
        script.push_str(&format!(
            r#"cat > /etc/update-motd.d/99-ec2-cli << 'MOTDEOF'
#!/bin/bash

# Gather system info
LOAD=$(awk '{{print $1}}' /proc/loadavg)
MEM_TOTAL=$(grep MemTotal /proc/meminfo | awk '{{print $2}}')
MEM_AVAIL=$(grep MemAvailable /proc/meminfo | awk '{{print $2}}')
MEM_PCT=$((100 - (MEM_AVAIL * 100 / MEM_TOTAL)))
DISK_PCT=$(df / | awk 'NR==2 {{gsub(/%/,""); print $5}}')
IP_ADDR=$(hostname -I | awk '{{print $1}}')
IFACE=$(ip route | awk '/default/ {{print $5}}' | head -1)

cat << EOF
┌──────────────────────────────────────────────────────────────────┐
│                                                                  │
│   ec2-cli Development Instance                                   │
│                                                                  │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│   System       Load: $LOAD    Memory: $MEM_PCT%    Disk: $DISK_PCT%
│   Network      $IP_ADDR ($IFACE)
│                                                                  │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│   Your Project ~/work/{}
│                (this is where \`ec2-cli push\` writes)             │
│                                                                  │
│   Workflow     1. In your repository, make changes and commit:   │
│                   git add . && git commit -m "message"           │
│                                                                  │
│                2. From your local machine:                       │
│                   ec2-cli pull                                   │
│                                                                  │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│   Logs         cat /var/log/ec2-cli-init.log                     │
│   Ready?       ls ~/.ec2-cli-ready                               │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
EOF
MOTDEOF
"#,
            name
        ));
        script.push_str("chmod +x /etc/update-motd.d/99-ec2-cli\n\n");

        // Create marker file to signal git repo is ready
        script.push_str(&format!("touch /home/{}/.ec2-cli-git-ready\n\n", username));
    }

    // Ensure SSM agent is running (pre-installed on Ubuntu 18.04+ AMIs)
    // Handle both snap-based (Ubuntu 18.04+) and deb-based (older/custom AMIs) installations
    script.push_str("echo 'Ensuring SSM agent is running...'\n");
    script.push_str("if snap list amazon-ssm-agent 2>/dev/null; then\n");
    script.push_str("    snap start amazon-ssm-agent 2>/dev/null || true\n");
    script.push_str(
        "    systemctl enable snap.amazon-ssm-agent.amazon-ssm-agent.service 2>/dev/null || true\n",
    );
    script.push_str(
        "    systemctl start snap.amazon-ssm-agent.amazon-ssm-agent.service 2>/dev/null || true\n",
    );
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
    // Note: docker group and user membership already configured earlier in the script
    script.push_str("echo 'Installing Docker...'\n");
    script.push_str("apt-get install -y docker.io\n");
    script.push_str("systemctl enable docker\n");
    script.push_str("systemctl start docker\n\n");

    // Install Rust if enabled
    if profile.packages.rust.enabled {
        // Validate rust components
        for component in &profile.packages.rust.components {
            validate_shell_safe(component, "rust component")?;
        }

        script.push_str("echo 'Installing Rust...'\n");
        script.push_str(&format!("su - {} -c '\n", username));
        script
            .push_str("curl --proto \"=https\" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y");

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

    // Install Claude Code CLI
    script.push_str("echo 'Installing Claude Code CLI...'\n");
    script.push_str(&format!(
        "su - {} -c 'curl -fsSL https://claude.ai/install.sh | bash'\n\n",
        username
    ));

    // Install AgentFS (requires lifting AppArmor restrictions for unprivileged user namespaces)
    script.push_str("echo 'Configuring AppArmor for AgentFS...'\n");
    script.push_str("cat > /etc/sysctl.d/99-agentfs.conf << 'AGENTFSEOF'\n");
    script.push_str("kernel.apparmor_restrict_unprivileged_userns = 0\n");
    script.push_str("AGENTFSEOF\n");
    script.push_str("sysctl -p /etc/sysctl.d/99-agentfs.conf\n\n");

    script.push_str("echo 'Installing AgentFS...'\n");
    script.push_str(&format!(
        "su - {} -c 'curl -fsSL https://agentfs.ai/install.sh | bash'\n\n",
        username
    ));

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
            generate_user_data(&profile, Some("test-project"), "ubuntu", None, None).unwrap();

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
        let script = generate_user_data(&profile, None, "ubuntu", None, None).unwrap();

        assert!(script.contains("#!/bin/bash"));
        assert!(!script.contains("git init --bare"));
        assert!(script.contains("docker.io"));
    }

    #[test]
    fn test_generate_with_ubuntu_user() {
        let profile = Profile::default_profile();
        let script = generate_user_data(&profile, Some("myproject"), "ubuntu", None, None).unwrap();

        assert!(script.contains("su - ubuntu"));
        assert!(script.contains("/home/ubuntu/"));
        assert!(!script.contains("ec2-user"));
    }

    #[test]
    fn test_generate_with_ssh_key() {
        let profile = Profile::default_profile();
        // Use a realistic key length (at least 50 chars base64)
        let ssh_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx user@example.com";
        let script = generate_user_data(
            &profile,
            Some("test-project"),
            "ubuntu",
            Some(ssh_key),
            None,
        )
        .unwrap();

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
        let script = generate_user_data(&profile, None, "ubuntu", None, None).unwrap();

        assert!(!script.contains("Configuring SSH public key"));
        assert!(!script.contains("authorized_keys"));
    }

    #[test]
    fn test_ssh_key_injected_before_package_installation() {
        // Ensure SSH key is available immediately when SSM reports ready,
        // not after package installation (which can take minutes)
        let profile = Profile::default_profile();
        let ssh_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx user@example.com";
        let script = generate_user_data(
            &profile,
            Some("test-project"),
            "ubuntu",
            Some(ssh_key),
            None,
        )
        .unwrap();

        let ssh_config_pos = script
            .find("Configuring SSH public key")
            .expect("SSH config not found");
        let package_install_pos = script
            .find("Installing system packages")
            .expect("package installation not found");

        assert!(
            ssh_config_pos < package_install_pos,
            "SSH key setup must occur before package installation to avoid race condition"
        );
    }

    #[test]
    fn test_git_ready_marker_created_after_repo_setup() {
        let profile = Profile::default_profile();
        let script =
            generate_user_data(&profile, Some("test-project"), "ubuntu", None, None).unwrap();

        let repo_setup_pos = script.find("git init --bare").expect("git init not found");
        let marker_pos = script.find(".ec2-cli-git-ready").expect("marker not found");

        assert!(
            marker_pos > repo_setup_pos,
            "Marker file must be created after git repo setup"
        );
    }

    #[test]
    fn test_docker_group_setup_before_package_installation() {
        let profile = Profile::default_profile();
        let script =
            generate_user_data(&profile, Some("test-project"), "ubuntu", None, None).unwrap();

        let docker_group_pos = script
            .find("Setting up docker group")
            .expect("docker group setup not found");
        let package_install_pos = script
            .find("Installing system packages")
            .expect("package installation not found");

        assert!(
            docker_group_pos < package_install_pos,
            "Docker group setup must occur before package installation"
        );
    }

    #[test]
    fn test_docker_group_uses_force_flag() {
        let profile = Profile::default_profile();
        let script =
            generate_user_data(&profile, Some("test-project"), "ubuntu", None, None).unwrap();

        assert!(
            script.contains("groupadd -f docker"),
            "groupadd should use -f flag for idempotency"
        );
    }

    #[test]
    fn test_generate_with_git_user_config() {
        let profile = Profile::default_profile();
        let git_config = GitUserConfig {
            name: Some("John Doe".to_string()),
            email: Some("john@example.com".to_string()),
        };
        let script = generate_user_data(
            &profile,
            Some("test-project"),
            "ubuntu",
            None,
            Some(&git_config),
        )
        .unwrap();

        assert!(script.contains("Configuring git user identity"));
        assert!(script.contains("git config --global user.name \"John Doe\""));
        assert!(script.contains("git config --global user.email \"john@example.com\""));
    }

    #[test]
    fn test_generate_with_name_only_git_config() {
        let profile = Profile::default_profile();
        let git_config = GitUserConfig {
            name: Some("John Doe".to_string()),
            email: None,
        };
        let script = generate_user_data(
            &profile,
            Some("test-project"),
            "ubuntu",
            None,
            Some(&git_config),
        )
        .unwrap();

        assert!(script.contains("git config --global user.name \"John Doe\""));
        assert!(!script.contains("git config --global user.email"));
    }

    #[test]
    fn test_generate_with_email_only_git_config() {
        let profile = Profile::default_profile();
        let git_config = GitUserConfig {
            name: None,
            email: Some("john@example.com".to_string()),
        };
        let script = generate_user_data(
            &profile,
            Some("test-project"),
            "ubuntu",
            None,
            Some(&git_config),
        )
        .unwrap();

        assert!(!script.contains("git config --global user.name"));
        assert!(script.contains("git config --global user.email \"john@example.com\""));
    }

    #[test]
    fn test_generate_without_git_config() {
        let profile = Profile::default_profile();
        let script =
            generate_user_data(&profile, Some("test-project"), "ubuntu", None, None).unwrap();

        assert!(!script.contains("Configuring git user identity"));
    }

    #[test]
    fn test_git_config_injection_blocked() {
        let profile = Profile::default_profile();
        let git_config = GitUserConfig {
            name: Some("John; rm -rf /".to_string()),
            email: None,
        };
        let result = generate_user_data(
            &profile,
            Some("test-project"),
            "ubuntu",
            None,
            Some(&git_config),
        );

        assert!(result.is_err());
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

        let result = generate_user_data(&profile, None, "ubuntu", None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_shell_injection_in_env_vars() {
        let mut profile = Profile::default_profile();
        profile
            .environment
            .insert("MALICIOUS".to_string(), "$(cat /etc/passwd)".to_string());

        let result = generate_user_data(&profile, None, "ubuntu", None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_agentfs_installed_by_default() {
        let profile = Profile::default_profile();
        let script = generate_user_data(&profile, None, "ubuntu", None, None).unwrap();

        // Check AppArmor configuration
        assert!(script.contains("/etc/sysctl.d/99-agentfs.conf"));
        assert!(script.contains("kernel.apparmor_restrict_unprivileged_userns = 0"));
        assert!(script.contains("sysctl -p /etc/sysctl.d/99-agentfs.conf"));

        // Check AgentFS installation
        assert!(script.contains("Installing AgentFS"));
        assert!(script.contains("agentfs.ai/install.sh"));
    }
}
