use std::process::Command;

use aws_sdk_ec2::types::Filter;
use dialoguer::{Input, Select};

use crate::aws::client::{get_default_vpc, AwsClients};
use crate::config::Settings;
use crate::profile::ProfileLoader;
use crate::ui::create_spinner;
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

    // Check AWS Credentials and get default region
    print!("  AWS Credentials: ");
    let aws_default_region = match AwsClients::new_without_settings().await {
        Ok(clients) => {
            println!("OK");
            println!("    Region: {}", clients.region);
            println!("    Account: {}", clients.account_id);
            Some(clients.region)
        }
        Err(_) => {
            println!("MISSING/INVALID");
            println!("    Configure with: aws configure");
            all_ok = false;
            None
        }
    };

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

    // If prerequisites failed, stop here
    if !all_ok {
        return Err(Ec2CliError::Prerequisites(
            "Some prerequisites are not met".to_string(),
        ));
    }

    // Load existing settings
    let mut settings = Settings::load().unwrap_or_default();

    println!("Configure ec2-cli settings:\n");

    // Configure region
    let default_region = settings
        .region
        .clone()
        .or(aws_default_region)
        .unwrap_or_else(|| "us-east-1".to_string());

    let region: String = Input::new()
        .with_prompt("  Region")
        .default(default_region)
        .interact_text()
        .map_err(|e| Ec2CliError::Config(format!("Failed to read input: {}", e)))?;

    // Validate region format
    Settings::validate_region(&region)?;

    settings.region = Some(region.clone());

    // Create clients with the selected region
    let spinner = create_spinner("Connecting to AWS...");
    let clients = AwsClients::with_region(&region).await.map_err(|e| {
        spinner.finish_and_clear();
        Ec2CliError::Config(format!(
            "Failed to connect to AWS in region '{}': {}",
            region, e
        ))
    })?;
    spinner.finish_and_clear();

    // Configure VPC
    let spinner = create_spinner("Looking up VPC...");
    let default_vpc_id = get_default_vpc(&clients).await.ok();
    spinner.finish_and_clear();
    let current_vpc = settings.vpc_id.clone().or(default_vpc_id.clone());

    let vpc_prompt = if let Some(ref vpc) = current_vpc {
        format!("  VPC [{}]", vpc)
    } else {
        "  VPC".to_string()
    };

    let vpc_input: String = Input::new()
        .with_prompt(&vpc_prompt)
        .default(current_vpc.unwrap_or_default())
        .allow_empty(false)
        .interact_text()
        .map_err(|e| Ec2CliError::Config(format!("Failed to read input: {}", e)))?;

    // Validate VPC exists
    let vpc_id = if vpc_input.is_empty() {
        default_vpc_id.clone().ok_or(Ec2CliError::NoDefaultVpc)?
    } else {
        // Validate format before API call
        Settings::validate_vpc_id(&vpc_input)?;
        let spinner = create_spinner("Validating VPC...");
        validate_vpc(&clients, &vpc_input).await?;
        spinner.finish_and_clear();
        vpc_input
    };

    // Store None if using default VPC, otherwise store the VPC ID
    settings.vpc_id = if Some(&vpc_id) == default_vpc_id.as_ref() {
        None
    } else {
        Some(vpc_id.clone())
    };

    // Configure subnet - list available subnets in the VPC
    let spinner = create_spinner("Fetching subnets...");
    let subnets = list_subnets(&clients, &vpc_id).await?;
    spinner.finish_and_clear();
    if subnets.is_empty() {
        return Err(Ec2CliError::NoSubnetsInVpc(vpc_id));
    }

    let subnet_options: Vec<String> = subnets
        .iter()
        .map(|s| {
            format!(
                "{} ({}, {})",
                s.subnet_id, s.availability_zone, s.cidr_block
            )
        })
        .collect();

    // Find current selection index
    let current_index = settings
        .subnet_id
        .as_ref()
        .and_then(|sid| subnets.iter().position(|s| &s.subnet_id == sid))
        .unwrap_or(0);

    println!();
    let selection = Select::new()
        .with_prompt("  Select subnet")
        .items(&subnet_options)
        .default(current_index)
        .interact()
        .map_err(|e| Ec2CliError::Config(format!("Failed to read input: {}", e)))?;

    settings.subnet_id = Some(subnets[selection].subnet_id.clone());

    // Configure Username tag
    println!();
    if settings.has_username_tag() {
        println!(
            "  Username tag: {} (already configured)",
            settings.tags.get("Username").unwrap()
        );
    } else {
        let username: String = Input::new()
            .with_prompt("  Enter your username (for resource tagging)")
            .interact_text()
            .map_err(|e| Ec2CliError::Config(format!("Failed to read input: {}", e)))?;

        settings.set_tag("Username", &username)?;
    }

    // Save settings
    settings.save()?;

    println!();
    println!("Configuration saved! You can now use 'ec2-cli up' to launch an instance.");

    Ok(())
}

/// Subnet info for display
struct SubnetInfo {
    subnet_id: String,
    availability_zone: String,
    cidr_block: String,
}

/// Validate that a VPC exists
async fn validate_vpc(clients: &AwsClients, vpc_id: &str) -> Result<()> {
    let vpcs = clients
        .ec2
        .describe_vpcs()
        .vpc_ids(vpc_id)
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    if vpcs.vpcs().is_empty() {
        return Err(Ec2CliError::VpcNotFound(vpc_id.to_string()));
    }

    Ok(())
}

/// List subnets in a VPC
async fn list_subnets(clients: &AwsClients, vpc_id: &str) -> Result<Vec<SubnetInfo>> {
    let subnets = clients
        .ec2
        .describe_subnets()
        .filters(Filter::builder().name("vpc-id").values(vpc_id).build())
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    Ok(subnets
        .subnets()
        .iter()
        .map(|s| SubnetInfo {
            subnet_id: s.subnet_id().unwrap_or_default().to_string(),
            availability_zone: s.availability_zone().unwrap_or_default().to_string(),
            cidr_block: s.cidr_block().unwrap_or_default().to_string(),
        })
        .collect())
}

pub fn show() -> Result<()> {
    let loader = ProfileLoader::new();
    let settings = Settings::load().unwrap_or_default();

    println!("Configuration:");
    println!();

    // AWS settings
    println!("AWS settings:");
    println!(
        "  Region: {}",
        settings.region.as_deref().unwrap_or("(from AWS config)")
    );
    println!(
        "  VPC: {}",
        settings.vpc_id.as_deref().unwrap_or("(default VPC)")
    );
    println!(
        "  Subnet: {}",
        settings
            .subnet_id
            .as_deref()
            .unwrap_or("(not configured - run 'ec2-cli config init')")
    );

    // Profile directories
    println!();
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

    // Custom tags
    println!();
    println!("Custom tags:");
    if settings.tags.is_empty() {
        println!("  (none configured)");
    } else {
        for (key, value) in &settings.tags {
            println!("  {}={}", key, value);
        }
    }

    Ok(())
}

/// Set a custom tag
pub fn tags_set(key: &str, value: &str) -> Result<()> {
    let mut settings = Settings::load()?;
    settings.set_tag(key, value)?;
    settings.save()?;
    println!("Tag '{}' set to '{}'", key, value);
    Ok(())
}

/// List all custom tags
pub fn tags_list() -> Result<()> {
    let settings = Settings::load()?;

    if settings.tags.is_empty() {
        println!("No custom tags configured.");
        println!();
        println!("Set a tag with: ec2-cli config tags set <KEY> <VALUE>");
    } else {
        println!("Custom tags:");
        for (key, value) in &settings.tags {
            println!("  {}={}", key, value);
        }
    }

    Ok(())
}

/// Remove a custom tag
pub fn tags_remove(key: &str) -> Result<()> {
    let mut settings = Settings::load()?;

    if settings.remove_tag(key).is_some() {
        settings.save()?;
        println!("Tag '{}' removed", key);
    } else {
        println!("Tag '{}' not found", key);
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
        Err(Ec2CliError::Prerequisites(
            "AWS CLI not working".to_string(),
        ))
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
