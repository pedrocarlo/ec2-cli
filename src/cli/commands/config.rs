use std::process::Command;

use crate::aws::client::AwsClients;
use crate::git::{check_ssh_config, generate_ssh_config_block, SshConfigStatus};
use crate::profile::ProfileLoader;
use crate::{Ec2CliError, Result};

pub async fn init() -> Result<()> {
    println!("Checking prerequisites...\n");

    let mut all_ok = true;

    // Check AWS CLI
    print!("  AWS CLI: ");
    match check_aws_cli() {
        Ok(version) => println!("OK ({})", version),
        Err(e) => {
            println!("MISSING");
            println!("    {}", e);
            all_ok = false;
        }
    }

    // Check Session Manager Plugin
    print!("  Session Manager Plugin: ");
    match check_session_manager_plugin() {
        Ok(version) => println!("OK ({})", version),
        Err(e) => {
            println!("MISSING");
            println!("    {}", e);
            println!("    Install from: https://docs.aws.amazon.com/systems-manager/latest/userguide/session-manager-working-with-install-plugin.html");
            all_ok = false;
        }
    }

    // Check AWS Credentials
    print!("  AWS Credentials: ");
    match AwsClients::new().await {
        Ok(clients) => {
            println!("OK");
            println!("    Region: {}", clients.region);
            println!("    Account: {}", clients.account_id);
        }
        Err(_) => {
            println!("MISSING/INVALID");
            println!("    Configure with: aws configure");
            all_ok = false;
        }
    }

    // Check SSH Config
    print!("  SSH Config: ");
    match check_ssh_config() {
        Ok(SshConfigStatus::Configured) => println!("OK"),
        Ok(SshConfigStatus::NeedsConfiguration) => {
            println!("NEEDS CONFIGURATION");
            println!("    Add the following to ~/.ssh/config:\n");
            println!("{}", generate_ssh_config_block());
            all_ok = false;
        }
        Ok(SshConfigStatus::Missing) => {
            println!("MISSING");
            println!("    Create ~/.ssh/config with:\n");
            println!("{}", generate_ssh_config_block());
            all_ok = false;
        }
        Err(e) => {
            println!("ERROR: {}", e);
            all_ok = false;
        }
    }

    // Check Git
    print!("  Git: ");
    match check_git() {
        Ok(version) => println!("OK ({})", version),
        Err(e) => {
            println!("MISSING");
            println!("    {}", e);
            all_ok = false;
        }
    }

    println!();

    if all_ok {
        println!("All prerequisites met! You can now use 'ec2-cli up' to launch an instance.");
        Ok(())
    } else {
        Err(Ec2CliError::Prerequisites(
            "Some prerequisites are not met".to_string(),
        ))
    }
}

pub fn show() -> Result<()> {
    let loader = ProfileLoader::new();

    println!("Configuration:");
    println!();

    // Profile directories
    println!("Profile directories:");
    if let Some(global_dir) = loader.global_dir() {
        println!("  Global: {}", global_dir.display());
    }
    if let Some(local_dir) = loader.local_dir() {
        println!("  Local: {}", local_dir.display());
    }

    // State file
    let state_dir = directories::ProjectDirs::from("", "", "ec2-cli")
        .and_then(|dirs| dirs.state_dir().map(|d| d.to_path_buf()));

    println!();
    println!("State directory:");
    if let Some(dir) = state_dir {
        println!("  {}", dir.display());
    } else {
        println!("  ~/.local/state/ec2-cli/");
    }

    // Available profiles
    println!();
    println!("Available profiles:");
    match loader.list() {
        Ok(profiles) => {
            for info in profiles {
                println!("  {} ({})", info.name, info.source);
            }
        }
        Err(e) => {
            println!("  Error listing profiles: {}", e);
        }
    }

    Ok(())
}

fn check_aws_cli() -> Result<String> {
    let output = Command::new("aws")
        .arg("--version")
        .output()
        .map_err(|_| Ec2CliError::Prerequisites("AWS CLI not found".to_string()))?;

    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .take(1)
            .collect::<String>();
        Ok(version)
    } else {
        Err(Ec2CliError::Prerequisites("AWS CLI not working".to_string()))
    }
}

fn check_session_manager_plugin() -> Result<String> {
    let output = Command::new("session-manager-plugin")
        .arg("--version")
        .output()
        .map_err(|_| Ec2CliError::SessionManagerPluginNotFound)?;

    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(version)
    } else {
        Err(Ec2CliError::SessionManagerPluginNotFound)
    }
}

fn check_git() -> Result<String> {
    let output = Command::new("git")
        .arg("--version")
        .output()
        .map_err(|_| Ec2CliError::Prerequisites("Git not found".to_string()))?;

    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout)
            .trim()
            .replace("git version ", "");
        Ok(version)
    } else {
        Err(Ec2CliError::Prerequisites("Git not working".to_string()))
    }
}
