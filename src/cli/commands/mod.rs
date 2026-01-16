pub mod config;
pub mod destroy;
pub mod list;
pub mod logs;
pub mod pull;
pub mod push;
pub mod scp;
pub mod ssh;
pub mod status;
pub mod up;

/// Returns the SSH command string for use with GIT_SSH_COMMAND environment variable.
/// This routes git SSH connections through AWS SSM Session Manager.
///
/// If `ssh_key_path` is provided, adds `-i <path>` to specify the identity file.
pub fn ssm_ssh_command(ssh_key_path: Option<&str>) -> String {
    // Escape single quotes in path for shell safety (replace ' with '\'' which ends the
    // quoted string, adds an escaped quote, and starts a new quoted string)
    let identity_flag = ssh_key_path
        .map(|path| format!("-i '{}' ", path.replace('\'', "'\\''")))
        .unwrap_or_default();

    format!(
        "ssh {}{}{}{}",
        identity_flag,
        "-o 'ProxyCommand=sh -c \"aws ssm start-session --target %h ",
        "--document-name AWS-StartSSHSession --parameters portNumber=%p\"' ",
        "-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"
    )
}
