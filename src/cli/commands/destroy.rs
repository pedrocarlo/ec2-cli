use dialoguer::Confirm;

use crate::aws::client::AwsClients;
use crate::aws::ec2::instance::{delete_security_group, terminate_instance, wait_for_terminated};
use crate::git::detect_vcs;
use crate::state::{get_instance, remove_instance as remove_instance_state, resolve_instance_name};
use crate::ui::create_spinner;
use crate::{Ec2CliError, Result};

/// Timeout for waiting for instance termination (seconds)
const TERMINATION_TIMEOUT_SECS: u64 = 120;

pub async fn execute(name: String, force: bool) -> Result<()> {
    // Resolve instance name
    let name = resolve_instance_name(Some(&name))?;

    // Get instance from state
    let instance_state =
        get_instance(&name)?.ok_or_else(|| Ec2CliError::InstanceNotFound(name.clone()))?;

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
    let spinner = create_spinner(format!(
        "Terminating EC2 instance {}...",
        instance_state.instance_id
    ));
    terminate_instance(&clients, &instance_state.instance_id).await?;
    spinner.finish_with_message(format!(
        "Instance {} terminating",
        instance_state.instance_id
    ));

    // Wait for instance to fully terminate before deleting security group
    // The ENI isn't released until the instance reaches "terminated" state
    let spinner = create_spinner("Waiting for instance to terminate...");
    wait_for_terminated(
        &clients,
        &instance_state.instance_id,
        TERMINATION_TIMEOUT_SECS,
    )
    .await?;
    spinner.finish_with_message(format!(
        "Instance {} terminated",
        instance_state.instance_id
    ));

    // Remove from state early - instance is confirmed terminated
    // This makes the operation more resilient if cleanup steps fail or crash
    remove_instance_state(&name)?;

    // Best-effort security group cleanup
    if let Some(ref sg_id) = instance_state.security_group_id {
        let spinner = create_spinner(format!("Deleting security group {}...", sg_id));
        match delete_security_group(&clients, sg_id).await {
            Ok(_) => {
                spinner.finish_with_message(format!("Security group {} deleted", sg_id));
            }
            Err(e) => {
                spinner.finish_with_message(format!(
                    "Warning: Could not delete security group {}: {}",
                    sg_id, e
                ));
            }
        }
    }

    // Try to remove git remote if it exists
    let remote_name = format!("ec2-{}", name);
    if let Some(vcs) = detect_vcs() {
        if let Ok(remotes) = vcs.list_remotes() {
            if remotes.contains(&remote_name) {
                println!("  Removing {} remote '{}'...", vcs.vcs_type(), remote_name);
                let _ = vcs.remove_remote(&remote_name);
            }
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
