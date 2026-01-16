use crate::git::{add_remote, git_pull, is_git_repo, list_remotes};
use crate::state::{get_instance, resolve_instance_name};
use crate::{Ec2CliError, Result};

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

    // Get project name from current directory
    let project_name = std::env::current_dir()?
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .ok_or_else(|| Ec2CliError::InvalidPath("Cannot determine project name".to_string()))?;

    // Use instance name as remote name
    let remote_name = format!("ec2-{}", name);

    // Add remote if it doesn't exist
    let remotes = list_remotes()?;
    if !remotes.contains(&remote_name) {
        let remote_url = format!(
            "ec2-user@{}:/home/ec2-user/repos/{}.git",
            instance_state.instance_id, project_name
        );
        println!("Adding remote '{}': {}", remote_name, remote_url);
        add_remote(&remote_name, &remote_url)?;
    }

    // Pull from remote
    println!("Pulling from {}...", remote_name);
    git_pull(&remote_name, branch.as_deref())?;

    println!("Pull complete!");
    Ok(())
}
