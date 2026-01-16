use crate::aws::client::AwsClients;
use crate::aws::ec2::instance::get_instance_state;
use crate::state::{get_instance, resolve_instance_name};
use crate::ui::create_spinner;
use crate::{Ec2CliError, Result};

pub async fn execute(name: Option<String>) -> Result<()> {
    // Resolve instance name
    let name = resolve_instance_name(name.as_deref())?;

    // Get instance from state
    let instance_state = get_instance(&name)?
        .ok_or_else(|| Ec2CliError::InstanceNotFound(name.clone()))?;

    println!("Instance: {}", name);
    println!("  Instance ID: {}", instance_state.instance_id);
    println!("  Profile: {}", instance_state.profile);
    println!("  Region: {}", instance_state.region);
    println!("  Created: {}", instance_state.created_at.format("%Y-%m-%d %H:%M:%S UTC"));

    // Get live status from AWS
    let spinner = create_spinner("Fetching instance status...");
    let clients = AwsClients::with_region(&instance_state.region).await?;

    match get_instance_state(&clients, &instance_state.instance_id).await {
        Ok(state) => {
            spinner.finish_and_clear();
            println!("  State: {:?}", state);
        }
        Err(e) => {
            spinner.finish_and_clear();
            println!("  State: unknown ({})", e);
        }
    }

    // Check for directory link
    let link_file = std::env::current_dir()
        .ok()
        .map(|p| p.join(".ec2-cli").join("instance"));

    if let Some(link_path) = link_file {
        if link_path.exists() {
            if let Ok(linked_name) = std::fs::read_to_string(&link_path) {
                if linked_name.trim() == name {
                    println!("  Linked: yes (current directory)");
                }
            }
        }
    }

    Ok(())
}
