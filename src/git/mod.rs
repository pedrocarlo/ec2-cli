pub mod operations;
pub mod remote;

pub use operations::{
    add_remote, get_current_branch, get_remote_url, git_pull, git_push, is_git_repo, list_remotes,
    remove_remote,
};
pub use remote::{check_ssh_config, generate_ssh_config_block, get_project_name, SshConfigStatus};
