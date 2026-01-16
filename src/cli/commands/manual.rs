/// Manual content for ec2-cli
const MANUAL_CONTENT: &str = r#"EC2-CLI(1)                     User Manual                     EC2-CLI(1)

NAME
    ec2-cli - Ephemeral EC2 Development Environment Manager

SYNOPSIS
    ec2-cli <command> [options]
    ec2-cli up [-p <profile>] [-n <name>] [-l]
    ec2-cli destroy <name> [-f]
    ec2-cli ssh <name> [-c <command>]
    ec2-cli scp <name> <src> <dest> [-r]
    ec2-cli push <name> [-b <branch>]
    ec2-cli pull <name> [-b <branch>]
    ec2-cli status [name]
    ec2-cli list [-a]
    ec2-cli logs <name> [-f]
    ec2-cli profile <subcommand>
    ec2-cli config <subcommand>
    ec2-cli manual

DESCRIPTION
    ec2-cli is a command-line tool for managing ephemeral EC2 development
    environments. It automates the lifecycle of temporary EC2 instances
    optimized for remote development work.

    Key features:
      - Launch pre-configured development instances with a single command
      - Secure access via AWS SSM Session Manager (no public IPs or SSH keys)
      - Git-based code synchronization with bare repositories
      - Customizable instance profiles in JSON5 format
      - Automatic resource cleanup and tagging

GETTING STARTED
    1. Ensure prerequisites are installed:
       - AWS CLI v2 configured with valid credentials
       - Session Manager plugin for AWS CLI
       - Git (for push/pull commands)

    2. Initialize ec2-cli and verify configuration:
       $ ec2-cli config init

    3. Set your username tag (recommended for resource tracking):
       $ ec2-cli config tags set Username "your.name"

    4. Launch your first instance:
       $ ec2-cli up -n mydev

    5. Connect via SSH:
       $ ec2-cli ssh mydev

    6. When finished, destroy the instance:
       $ ec2-cli destroy mydev

COMMANDS
    up [-p <profile>] [-n <name>] [-l]
        Launch a new EC2 instance.

        Options:
            -p, --profile <name>    Profile to use (default: "default")
            -n, --name <name>       Custom instance name (auto-generated if omitted)
            -l, --link              Link instance to current directory

        Examples:
            ec2-cli up                          # Launch with defaults
            ec2-cli up -p rust-dev              # Launch with custom profile
            ec2-cli up -n myproject -l          # Named instance, linked to pwd

    destroy <name> [-f]
        Terminate an instance and cleanup associated resources.

        Options:
            -f, --force             Skip confirmation prompt

        Examples:
            ec2-cli destroy mydev               # Interactive confirmation
            ec2-cli destroy mydev -f            # Force destroy

    ssh <name> [-c <command>]
        SSH into an instance via SSM Session Manager.

        Options:
            -c, --command <cmd>     Execute command instead of interactive shell

        Examples:
            ec2-cli ssh mydev                   # Interactive shell
            ec2-cli ssh mydev -c "uname -a"    # Run single command

    scp <name> <src> <dest> [-r]
        Copy files to/from an instance via SSM. Prefix remote paths with ":".

        Options:
            -r, --recursive         Copy directories recursively

        Examples:
            ec2-cli scp mydev ./file.txt :/home/ubuntu/
            ec2-cli scp mydev :/home/ubuntu/file.txt ./
            ec2-cli scp mydev -r ./project :/home/ubuntu/

    push <name> [-b <branch>]
        Push local git repository to the instance's bare repository.

        Options:
            -b, --branch <name>     Branch to push (default: current branch)

        Examples:
            ec2-cli push mydev                  # Push current branch
            ec2-cli push mydev -b feature       # Push specific branch

    pull <name> [-b <branch>]
        Pull from the instance's bare repository to local.

        Options:
            -b, --branch <name>     Branch to pull (default: current branch)

        Examples:
            ec2-cli pull mydev                  # Pull current branch
            ec2-cli pull mydev -b main          # Pull specific branch

    status [name]
        Show instance status. If no name given, uses linked instance.

        Examples:
            ec2-cli status mydev               # Named instance
            ec2-cli status                     # Linked instance

    list [-a]
        List all managed instances.

        Options:
            -a, --all               Include terminated instances

        Examples:
            ec2-cli list                       # Active instances only
            ec2-cli list -a                    # Include terminated

    logs <name> [-f]
        View cloud-init logs from an instance.

        Options:
            -f, --follow            Follow log output (like tail -f)

        Examples:
            ec2-cli logs mydev                 # View logs
            ec2-cli logs mydev -f              # Follow logs

    profile list
        List all available profiles.

    profile show <name>
        Display details of a specific profile.

    profile validate <name>
        Validate a profile's configuration.

    config init
        Initialize configuration and verify prerequisites.

    config show
        Display current configuration settings.

    config tags set <key> <value>
        Set a custom tag applied to all AWS resources.

    config tags list
        List all configured custom tags.

    config tags remove <key>
        Remove a custom tag.

    completions <shell>
        Generate shell completions (bash, zsh, fish).

        Examples:
            ec2-cli completions bash >> ~/.bashrc
            ec2-cli completions zsh >> ~/.zshrc

    manual
        Display this manual.

FILES
    ~/.config/ec2-cli/config.json
        Global configuration file containing custom tags, region override,
        VPC/subnet settings.

    ~/.config/ec2-cli/profiles/
        Directory for global profile definitions (JSON5 format).

    ~/.local/state/ec2-cli/state.json
        Local state file tracking active instances.

    .ec2-cli/profiles/
        Project-local profile directory (takes precedence over global).

    .ec2-cli/instance
        File containing linked instance name for the current directory.

PROFILES
    Profiles define instance configurations in JSON5 format. They specify
    instance type, AMI, storage, packages, and environment variables.

    Profile locations (searched in order):
      1. .ec2-cli/profiles/<name>.json5  (project-local)
      2. ~/.config/ec2-cli/profiles/<name>.json5  (global)
      3. Built-in "default" profile

    Schema:
        {
          name: "profile-name",
          instance: {
            type: "t3.large",              // EC2 instance type
            fallback_types: ["t3.medium"], // Fallback if primary unavailable
            ami: {
              type: "ubuntu-24.04",        // AMI type (ubuntu-22.04, ubuntu-24.04)
              architecture: "x86_64",      // x86_64 or arm64
              id: null                     // Optional specific AMI ID
            },
            storage: {
              root_volume: {
                size_gb: 30,               // 8-16384 GB
                type: "gp3",               // gp2, gp3, io1, io2, st1, sc1
                iops: 3000,                // For gp3/io1/io2
                throughput: 125            // For gp3 (MB/s)
              }
            }
          },
          packages: {
            system: ["build-essential", "git"],  // apt packages
            rust: {
              enabled: true,
              channel: "stable",           // stable, beta, nightly
              components: ["rustfmt", "clippy"]
            },
            cargo: ["cargo-watch"]         // Cargo packages to install
          },
          environment: {
            EDITOR: "vim"                  // Environment variables
          }
        }

    Example: Create ~/.config/ec2-cli/profiles/rust-dev.json5
        {
          name: "rust-dev",
          instance: {
            type: "c6i.xlarge",
            ami: { type: "ubuntu-24.04" },
            storage: { root_volume: { size_gb: 50 } }
          },
          packages: {
            system: ["build-essential", "libssl-dev", "pkg-config"],
            rust: { enabled: true, channel: "stable" },
            cargo: ["cargo-watch", "cargo-expand"]
          }
        }

SECURITY
    ec2-cli is designed with security in mind:

    Network Security:
      - Instances run in a VPC with no public IP address
      - No inbound security group rules (no open ports)
      - All access via AWS SSM Session Manager

    Instance Security:
      - IMDSv2 required (protects against SSRF attacks)
      - EBS volumes encrypted by default
      - No SSH keys stored or transmitted

    Credential Security:
      - Uses AWS SDK default credential chain
      - No credentials stored in state files
      - Session Manager handles authentication

    Resource Tagging:
      - All resources tagged with ec2-cli:managed=true
      - Custom tags for ownership tracking (e.g., Username)
      - Enables easy resource identification and cleanup

PREREQUISITES
    Required software:
      - AWS CLI v2 (https://aws.amazon.com/cli/)
      - Session Manager plugin (https://docs.aws.amazon.com/systems-manager/
        latest/userguide/session-manager-working-with-install-plugin.html)
      - Git (for push/pull commands)

    AWS permissions required:
      - ec2:* (instance management)
      - ssm:StartSession (SSM access)
      - iam:CreateRole, iam:AttachRolePolicy (one-time setup)
      - iam:CreateInstanceProfile (one-time setup)

    Run 'ec2-cli config init' to verify all prerequisites.

ENVIRONMENT VARIABLES
    AWS_REGION
        Override the default AWS region.

    AWS_PROFILE
        Use a specific AWS CLI profile.

    EC2_CLI_NO_COLOR
        Disable colored output when set to any value.

EXAMPLES
    Basic workflow:
        # Launch a development instance
        ec2-cli up -n dev -l

        # Connect and work
        ec2-cli ssh dev

        # Sync code
        ec2-cli push dev

        # Download results
        ec2-cli scp dev :/home/ubuntu/output.txt ./

        # Clean up
        ec2-cli destroy dev

    Using profiles:
        # Create a profile for data science work
        cat > ~/.config/ec2-cli/profiles/datasci.json5 << 'EOF'
        {
          name: "datasci",
          instance: {
            type: "r6i.xlarge",
            storage: { root_volume: { size_gb: 100 } }
          },
          packages: {
            system: ["python3-pip", "python3-venv"]
          }
        }
        EOF

        # Launch with the profile
        ec2-cli up -p datasci -n analysis

    Multiple instances:
        # Launch instances for different tasks
        ec2-cli up -n backend -p default
        ec2-cli up -n frontend -p default

        # List all
        ec2-cli list

        # Work on specific instance
        ec2-cli ssh backend

SEE ALSO
    AWS CLI: https://aws.amazon.com/cli/
    Session Manager: https://docs.aws.amazon.com/systems-manager/
    Source code: https://github.com/LeMikaelF/ec2-cli

VERSION
    ec2-cli 0.1.0

"#;

/// Execute the manual command - print comprehensive documentation
pub fn execute() {
    println!("{}", MANUAL_CONTENT);
}
