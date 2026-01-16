use crate::aws::client::AwsClients;
use crate::aws::ec2::instance::{
    create_instance_security_group, delete_security_group, launch_instance, wait_for_running,
    wait_for_ssm_ready,
};
use crate::aws::infrastructure::Infrastructure;
use crate::config::Settings;
use crate::profile::ProfileLoader;
use crate::ui::create_spinner;
use crate::user_data::{generate_user_data, validate_project_name};
use crate::Result;

/// Get the SSH username (always ubuntu for Ubuntu AMIs)
fn get_username_for_ami(_ami_type: &str) -> &'static str {
    "ubuntu"
}

pub async fn execute(
    profile_name: Option<String>,
    instance_name: Option<String>,
    link: bool,
) -> Result<()> {
    // Load profile
    let loader = ProfileLoader::new();
    let profile_name = profile_name.unwrap_or_else(|| "default".to_string());
    let profile = loader.load(&profile_name)?;
    profile.validate()?;

    // Generate instance name if not provided
    let name = instance_name.unwrap_or_else(|| {
        petname::petname(2, "-").unwrap_or_else(|| "ec2-instance".to_string())
    });

    // Determine username based on AMI type
    let username = get_username_for_ami(&profile.instance.ami.ami_type);

    println!("Launching EC2 instance '{}'...", name);
    println!("  Profile: {}", profile.name);
    println!("  Instance type: {}", profile.instance.instance_type);
    println!("  AMI type: {} (user: {})", profile.instance.ami.ami_type, username);

    // Initialize AWS clients
    let spinner = create_spinner("Connecting to AWS...");
    let clients = AwsClients::new().await?;
    spinner.finish_with_message("Connected to AWS");

    // Get or create infrastructure (VPC, subnet from config; IAM resources created if needed)
    let spinner = create_spinner("Checking infrastructure...");
    let infra = Infrastructure::get_or_create(&clients).await?;
    spinner.finish_with_message("Infrastructure ready");

    // Load custom tags for security group
    let custom_tags = Settings::load()
        .map(|s| s.tags)
        .unwrap_or_default();

    // Create per-instance security group
    let spinner = create_spinner("Creating security group...");
    let security_group_id =
        create_instance_security_group(&clients, &infra.vpc_id, &name, &custom_tags).await?;
    spinner.finish_with_message("Security group created");

    // Get project name from current directory (for git repo setup)
    let project_name = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

    // Validate project name if present
    if let Some(ref proj_name) = project_name {
        validate_project_name(proj_name)?;
    }

    // Generate user data
    let user_data = generate_user_data(&profile, project_name.as_deref(), username)?;

    // Launch instance (cleanup security group on failure)
    let spinner = create_spinner("Launching instance...");
    let instance_id = match launch_instance(
        &clients,
        &infra,
        &security_group_id,
        &profile,
        &name,
        &user_data,
    )
    .await
    {
        Ok(id) => {
            spinner.finish_with_message(format!("Instance launched: {}", id));
            id
        }
        Err(e) => {
            spinner.finish_and_clear();
            // Cleanup security group on launch failure
            let _ = delete_security_group(&clients, &security_group_id).await;
            return Err(e);
        }
    };

    // Wait for instance to be running
    let spinner = create_spinner("Waiting for instance to start...");
    wait_for_running(&clients, &instance_id, 300).await?;
    spinner.finish_with_message("Instance running");

    // Wait for SSM agent to be ready
    let spinner = create_spinner("Waiting for SSM agent...");
    wait_for_ssm_ready(&clients, &instance_id, 600).await?;
    spinner.finish_with_message("SSM agent ready");

    // Save state with username and security group ID
    crate::state::save_instance(
        &name,
        &instance_id,
        &profile.name,
        &clients.region,
        username,
        &security_group_id,
    )?;

    // Create link file if requested
    if link {
        create_link_file(&name)?;
        println!("  Linked to current directory");
    }

    println!();
    println!("Instance '{}' is ready!", name);
    println!("  Instance ID: {}", instance_id);
    println!("  Connect with: ec2-cli ssh {}", name);

    if let Some(ref proj) = project_name {
        println!("  Push code with: ec2-cli push {}", name);
        println!("  Git remote: {}@{}:/home/{}/repos/{}.git", username, instance_id, username, proj);
    }

    Ok(())
}

fn create_link_file(name: &str) -> Result<()> {
    let link_dir = std::env::current_dir()?.join(".ec2-cli");
    std::fs::create_dir_all(&link_dir)?;

    let link_file = link_dir.join("instance");
    std::fs::write(&link_file, name)?;

    Ok(())
}
