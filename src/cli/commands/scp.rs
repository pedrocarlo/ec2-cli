use std::process::Command;

use crate::ssh::SSM_PROXY_COMMAND;
use crate::state::{get_instance, resolve_instance_name};
use crate::{Ec2CliError, Result};

pub fn execute(name: String, src: String, dest: String, recursive: bool) -> Result<()> {
    // Resolve instance name
    let name = resolve_instance_name(Some(&name))?;

    // Get instance from state
    let instance_state = get_instance(&name)?
        .ok_or_else(|| Ec2CliError::InstanceNotFound(name.clone()))?;

    // Parse source and destination to determine direction
    let (local_path, remote_path, is_upload) = parse_paths(&src, &dest)?;

    let remote = format!(
        "{}@{}:{}",
        instance_state.username, instance_state.instance_id, remote_path
    );

    let mut cmd = Command::new("scp");

    // Add identity file if we have the SSH key path stored
    if let Some(ref key_path) = instance_state.ssh_key_path {
        cmd.arg("-i").arg(key_path);
    }

    cmd.arg("-o")
        .arg(format!("ProxyCommand={}", SSM_PROXY_COMMAND))
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null");

    if recursive {
        cmd.arg("-r");
    }

    if is_upload {
        cmd.arg(&local_path).arg(&remote);
    } else {
        cmd.arg(&remote).arg(&local_path);
    }

    let status = cmd
        .status()
        .map_err(|e| Ec2CliError::ScpTransfer(format!("Failed to execute scp: {}", e)))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

fn parse_paths(src: &str, dest: &str) -> Result<(String, String, bool)> {
    let src_is_remote = src.starts_with(':');
    let dest_is_remote = dest.starts_with(':');

    match (src_is_remote, dest_is_remote) {
        (false, true) => {
            // Upload: local src -> remote dest
            Ok((src.to_string(), dest[1..].to_string(), true))
        }
        (true, false) => {
            // Download: remote src -> local dest
            Ok((dest.to_string(), src[1..].to_string(), false))
        }
        (true, true) => Err(Ec2CliError::InvalidPath(
            "Both source and destination cannot be remote".to_string(),
        )),
        (false, false) => Err(Ec2CliError::InvalidPath(
            "One of source or destination must be remote (prefix with :)".to_string(),
        )),
    }
}
