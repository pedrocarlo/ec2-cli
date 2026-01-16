use std::process::{Command, Stdio};

use crate::state::{get_instance, resolve_instance_name};
use crate::{Ec2CliError, Result};

pub fn execute(name: String, follow: bool) -> Result<()> {
    // Resolve instance name
    let name = resolve_instance_name(Some(&name))?;

    // Get instance from state
    let instance_state =
        get_instance(&name)?.ok_or_else(|| Ec2CliError::InstanceNotFound(name.clone()))?;

    let instance_id = &instance_state.instance_id;
    let username = &instance_state.username;

    // Build command to view logs
    let cmd = if follow {
        "tail -f /var/log/ec2-cli-init.log"
    } else {
        "cat /var/log/ec2-cli-init.log"
    };

    println!("Viewing logs from {}...\n", name);

    let status = Command::new("ssh")
        .arg(format!("{}@{}", username, instance_id))
        .arg(cmd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| Ec2CliError::SshCommand(e.to_string()))?;

    if !status.success() && !follow {
        // Log file might not exist yet
        println!("\nNote: Log file may not exist yet if cloud-init hasn't started.");
    }

    Ok(())
}
