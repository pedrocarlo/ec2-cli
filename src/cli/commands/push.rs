use crate::git::{detect_vcs, PushOptions};
use crate::state::{get_instance, resolve_instance_name};
use crate::user_data::validate_project_name;
use crate::{Ec2CliError, Result};

use super::ssm_ssh_command;

pub fn execute(name: String, branch: Option<String>) -> Result<()> {
    // Detect which VCS is in use
    let vcs = detect_vcs().ok_or(Ec2CliError::NotGitRepo)?;

    // Resolve instance name
    let name = resolve_instance_name(Some(&name))?;

    // Get instance from state
    let instance_state =
        get_instance(&name)?.ok_or_else(|| Ec2CliError::InstanceNotFound(name.clone()))?;

    let username = &instance_state.username;

    // Get project name from current directory
    let project_name = std::env::current_dir()?
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .ok_or_else(|| Ec2CliError::InvalidPath("Cannot determine project name".to_string()))?;

    // Validate project name for security
    validate_project_name(&project_name)?;

    // Use instance name as remote name
    let remote_name = format!("ec2-{}", name);

    // Build the remote URL
    let remote_url = format!(
        "{}@{}:/home/{}/repos/{}.git",
        username, instance_state.instance_id, username, project_name
    );

    // Get SSH command for SSM
    let ssh_cmd = ssm_ssh_command(instance_state.ssh_key_path.as_deref());

    // Ensure remote exists
    if vcs.ensure_remote(&remote_name, &remote_url)? {
        println!("Adding remote '{}': {}", remote_name, remote_url);
    }

    // Get branch to push (use provided or detect current)
    let branch_to_push = match branch {
        Some(b) => Some(b),
        None => vcs.current_branch()?,
    };

    println!("Pushing to {} (using {})...", remote_name, vcs.vcs_type());
    vcs.push(
        &remote_name,
        PushOptions {
            branch: branch_to_push.as_deref(),
            set_upstream: true,
            ssh_command: Some(&ssh_cmd),
        },
    )?;

    println!("Push complete!");
    Ok(())
}
