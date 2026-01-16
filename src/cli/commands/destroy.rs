use dialoguer::Confirm;

use crate::aws::client::AwsClients;
use crate::aws::ec2::instance::terminate_instance;
use crate::git::{list_remotes, remove_remote};
use crate::state::{get_instance, remove_instance as remove_instance_state, resolve_instance_name};
use crate::{Ec2CliError, Result};

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
    let clients = AwsClients::with_region(&instance_state.region).await?;

    // Terminate the instance
    println!("  Terminating EC2 instance {}...", instance_state.instance_id);
    terminate_instance(&clients, &instance_state.instance_id).await?;

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
