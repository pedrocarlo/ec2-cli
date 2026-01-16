mod key_loader;

pub use key_loader::find_ssh_public_key;

/// SSM proxy command for SSH connections through Session Manager
pub const SSM_PROXY_COMMAND: &str =
    "sh -c \"aws ssm start-session --target %h --document-name AWS-StartSSHSession --parameters portNumber=%p\"";
