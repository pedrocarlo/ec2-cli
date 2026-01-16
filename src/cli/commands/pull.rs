use crate::git::{add_remote, git_pull, is_git_repo, list_remotes};
use crate::state::{get_instance, resolve_instance_name};
use crate::user_data::validate_project_name;
use crate::{Ec2CliError, Result};

use super::ssm_ssh_command;

pub fn execute(name: String, branch: Option<String>) -> Result<()> {
    // Check we're in a git repo
    if !is_git_repo() {
        return Err(Ec2CliError::NotGitRepo);
    }

    // Resolve instance name
    let name = resolve_instance_name(Some(&name))?;

    // Get instance from state
    let instance_state = get_instance(&name)?
        .ok_or_else(|| Ec2CliError::InstanceNotFound(name.clone()))?;

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

    // Add remote if it doesn't exist
    let remotes = list_remotes()?;
    if !remotes.contains(&remote_name) {
        let remote_url = format!(
            "{}@{}:/home/{}/repos/{}.git",
            username, instance_state.instance_id, username, project_name
        );
        println!("Adding remote '{}': {}", remote_name, remote_url);
        add_remote(&remote_name, &remote_url)?;
    }

    // Pull from remote with SSM SSH command (include identity file if available)
    let ssh_cmd = ssm_ssh_command(instance_state.ssh_key_path.as_deref());
    println!("Pulling from {}...", remote_name);
    git_pull(&remote_name, branch.as_deref(), Some(&ssh_cmd))?;

    println!("Pull complete!");
    Ok(())
}
