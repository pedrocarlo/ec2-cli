use crate::git::{
    add_remote, detect_vcs, git_pull, jj_add_remote, jj_fetch, jj_list_remotes, list_remotes,
    VcsType,
};
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

    match vcs {
        VcsType::JJ => {
            // Check if remote already exists
            let remotes = jj_list_remotes()?;
            if !remotes.contains(&remote_name) {
                println!("Adding remote '{}': {}", remote_name, remote_url);
                jj_add_remote(&remote_name, &remote_url)?;
            }

            // JJ uses fetch instead of pull (it auto-rebases)
            // Note: branch parameter is ignored for JJ fetch as it fetches all refs
            if branch.is_some() {
                println!(
                    "Note: JJ fetches all refs from remote, branch filter is not applied"
                );
            }

            println!("Fetching from {} (using jj)...", remote_name);
            jj_fetch(&remote_name, Some(&ssh_cmd))?;
        }
        VcsType::Git => {
            // Check if remote already exists
            let remotes = list_remotes()?;
            if !remotes.contains(&remote_name) {
                println!("Adding remote '{}': {}", remote_name, remote_url);
                add_remote(&remote_name, &remote_url)?;
            }

            println!("Pulling from {}...", remote_name);
            git_pull(&remote_name, branch.as_deref(), Some(&ssh_cmd))?;
        }
    }

    println!("Pull complete!");
    Ok(())
}
