use std::process::Command;

use crate::state::{get_instance, resolve_instance_name};
use crate::{Ec2CliError, Result};

const PROXY_COMMAND: &str =
    "sh -c \"aws ssm start-session --target %h --document-name AWS-StartSSHSession --parameters portNumber=%p\"";

pub fn execute(name: String, command: Option<String>) -> Result<()> {
    // Resolve instance name
    let name = resolve_instance_name(Some(&name))?;

    // Get instance from state
    let instance_state = get_instance(&name)?
        .ok_or_else(|| Ec2CliError::InstanceNotFound(name.clone()))?;

    let target = format!(
        "{}@{}",
        instance_state.username, instance_state.instance_id
    );

    let mut cmd = Command::new("ssh");
    cmd.arg("-o")
        .arg(format!("ProxyCommand={}", PROXY_COMMAND))
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg(&target);

    if let Some(remote_cmd) = command {
        cmd.arg(remote_cmd);
    }

    let status = cmd
        .status()
        .map_err(|e| Ec2CliError::SshCommand(format!("Failed to execute ssh: {}", e)))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}
