use crate::git::{
    add_remote, detect_vcs, git_push, jj_add_remote, jj_get_current_bookmark, jj_list_remotes,
    jj_push, list_remotes, VcsType,
};
use crate::state::{get_instance, resolve_instance_name};
use crate::user_data::validate_project_name;
use crate::{Ec2CliError, Result};
use std::process::Command;

use super::ssm_ssh_command;

/// Get the current git branch name
fn get_current_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| Ec2CliError::Git(e.to_string()))?;

    if !output.status.success() {
        return Err(Ec2CliError::Git("Failed to get current branch".to_string()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

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

            // Get bookmark to push (use provided or detect current)
            let bookmark_to_push = match branch {
                Some(b) => Some(b),
                None => jj_get_current_bookmark()?,
            };

            println!("Pushing to {} (using jj)...", remote_name);
            jj_push(&remote_name, bookmark_to_push.as_deref(), Some(&ssh_cmd))?;
        }
        VcsType::Git => {
            // Check if remote already exists
            let remotes = list_remotes()?;
            if !remotes.contains(&remote_name) {
                println!("Adding remote '{}': {}", remote_name, remote_url);
                add_remote(&remote_name, &remote_url)?;
            }

            // Get branch to push (use provided branch or current branch)
            let branch_to_push = match branch {
                Some(b) => b,
                None => get_current_branch()?,
            };

            println!("Pushing to {}...", remote_name);
            git_push(
                &remote_name,
                Some(&branch_to_push),
                true, // always set upstream
                Some(&ssh_cmd),
            )?;
        }
    }

    println!("Push complete!");
    Ok(())
}
