use dialoguer::Confirm;

use crate::aws::client::AwsClients;
use crate::aws::ec2::instance::{delete_security_group, terminate_instance};
use crate::git::{list_remotes, remove_remote};
use crate::state::{get_instance, remove_instance as remove_instance_state, resolve_instance_name};
use crate::ui::create_spinner;
use crate::{Ec2CliError, Result};

/// Initial wait time before attempting to delete security group (seconds)
const SG_DELETE_INITIAL_WAIT_SECS: u64 = 10;
/// Maximum number of attempts to delete security group
const SG_DELETE_MAX_ATTEMPTS: u32 = 6;
/// Wait time between retry attempts (seconds)
const SG_DELETE_RETRY_INTERVAL_SECS: u64 = 10;

pub async fn execute(name: String, force: bool) -> Result<()> {
    // Resolve instance name
    let name = resolve_instance_name(Some(&name))?;

    // Get instance from state
    let instance_state = get_instance(&name)?
        .ok_or_else(|| Ec2CliError::InstanceNotFound(name.clone()))?;

    // Confirm destruction unless forced
    if !force {
        let confirmed = Confirm::new()
            .with_prompt(format!(
                "Are you sure you want to destroy instance '{}'?",
                name
            ))
            .default(false)
            .interact()
            .map_err(|_| Ec2CliError::Cancelled)?;

        if !confirmed {
            println!("Cancelled.");
            return Ok(());
        }
    }

    println!("Destroying instance '{}'...", name);

    // Initialize AWS clients with the correct region
    let spinner = create_spinner("Connecting to AWS...");
    let clients = AwsClients::with_region(&instance_state.region).await?;
    spinner.finish_with_message("Connected to AWS");

    // Terminate the instance
    let spinner = create_spinner(format!("Terminating EC2 instance {}...", instance_state.instance_id));
    terminate_instance(&clients, &instance_state.instance_id).await?;
    spinner.finish_with_message(format!("Instance {} terminated", instance_state.instance_id));

    // Delete the security group (if present in state)
    // Note: We need to wait a bit for the instance to terminate before we can delete the SG
    if let Some(ref sg_id) = instance_state.security_group_id {
        let spinner = create_spinner("Waiting before cleanup...");
        // Wait for instance to terminate so SG can be deleted
        tokio::time::sleep(tokio::time::Duration::from_secs(SG_DELETE_INITIAL_WAIT_SECS)).await;
        spinner.finish_and_clear();

        let spinner = create_spinner(format!("Deleting security group {}...", sg_id));
        // Try a few times in case the instance hasn't fully terminated yet
        let mut attempts = 0;
        loop {
            match delete_security_group(&clients, sg_id).await {
                Ok(_) => {
                    spinner.finish_with_message(format!("Security group {} deleted", sg_id));
                    break;
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= SG_DELETE_MAX_ATTEMPTS {
                        spinner.finish_with_message(format!(
                            "Warning: Could not delete security group {}: {}",
                            sg_id, e
                        ));
                        break;
                    }
                    // Wait and retry
                    tokio::time::sleep(tokio::time::Duration::from_secs(
                        SG_DELETE_RETRY_INTERVAL_SECS,
                    ))
                    .await;
                }
            }
        }
    }

    // Remove from state
    remove_instance_state(&name)?;

    // Try to remove git remote if it exists
    let remote_name = format!("ec2-{}", name);
    if let Ok(remotes) = list_remotes() {
        if remotes.contains(&remote_name) {
            println!("  Removing git remote '{}'...", remote_name);
            let _ = remove_remote(&remote_name);
        }
    }

    // Remove link file if it exists and matches this instance
    let link_file = std::env::current_dir()
        .ok()
        .map(|p| p.join(".ec2-cli").join("instance"));

    if let Some(link_path) = link_file {
        if link_path.exists() {
            if let Ok(linked_name) = std::fs::read_to_string(&link_path) {
                if linked_name.trim() == name {
                    let _ = std::fs::remove_file(&link_path);
                    println!("  Removed directory link");
                }
            }
        }
    }

    println!("Instance '{}' destroyed.", name);
    Ok(())
}
