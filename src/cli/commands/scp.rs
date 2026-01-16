use std::process::{Command, Stdio};

use crate::state::{get_instance, resolve_instance_name};
use crate::{Ec2CliError, Result};

pub fn execute(name: String, src: String, dest: String, recursive: bool) -> Result<()> {
    // Resolve instance name
    let name = resolve_instance_name(Some(&name))?;

    // Get instance from state
    let instance_state = get_instance(&name)?
        .ok_or_else(|| Ec2CliError::InstanceNotFound(name.clone()))?;

    let instance_id = &instance_state.instance_id;

    // Parse source and destination to determine direction
    let (local_path, remote_path, is_upload) = parse_paths(&src, &dest)?;

    // Build SCP command
    let mut cmd = Command::new("scp");

    if recursive {
        cmd.arg("-r");
    }

    if is_upload {
        // Local to remote
        cmd.arg(&local_path);
        cmd.arg(format!("ec2-user@{}:{}", instance_id, remote_path));
    } else {
        // Remote to local
        cmd.arg(format!("ec2-user@{}:{}", instance_id, remote_path));
        cmd.arg(&local_path);
    }

    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| Ec2CliError::ScpTransfer(e.to_string()))?;

    if !status.success() {
        return Err(Ec2CliError::ScpTransfer(format!(
            "SCP failed with exit code: {:?}",
            status.code()
        )));
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
