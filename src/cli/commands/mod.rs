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
pub fn ssm_ssh_command() -> &'static str {
    concat!(
        "ssh -o 'ProxyCommand=sh -c \"aws ssm start-session --target %h ",
        "--document-name AWS-StartSSHSession --parameters portNumber=%p\"' ",
        "-o StrictHostKeyChecking=no ",
        "-o UserKnownHostsFile=/dev/null"
    )
}
