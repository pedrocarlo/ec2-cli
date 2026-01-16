use clap::{Parser, Subcommand};

mod aws;
mod cli;
mod config;
mod error;
mod git;
mod profile;
mod ssh;
mod state;
mod ui;
mod user_data;

pub use error::{Ec2CliError, Result};
pub use profile::{Profile, ProfileLoader};

#[derive(Parser)]
#[command(name = "ec2-cli")]
#[command(about = "Ephemeral EC2 Development Environment Manager")]
#[command(version)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch a new EC2 instance
    Up {
        /// Profile name to use (default if omitted)
        #[arg(short, long)]
        profile: Option<String>,

        /// Custom instance name
        #[arg(short, long)]
        name: Option<String>,

        /// Link instance to current directory
        #[arg(short, long)]
        link: bool,
    },

    /// Terminate instance and cleanup resources
    Destroy {
        /// Instance name
        name: String,

        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// SSH into instance via SSM Session Manager
    Ssh {
        /// Instance name
        name: String,

        /// Command to execute
        #[arg(short = 'c', long)]
        command: Option<String>,
    },

    /// Copy files to/from EC2 instance via SSM
    Scp {
        /// Instance name
        name: String,

        /// Source path (prefix with : for remote)
        src: String,

        /// Destination path (prefix with : for remote)
        dest: String,

        /// Copy directories recursively
        #[arg(short, long)]
        recursive: bool,
    },

    /// Push code to EC2 bare repo
    Push {
        /// Instance name
        name: String,

        /// Branch to push
        #[arg(short, long)]
        branch: Option<String>,
    },

    /// Pull from EC2 bare repo
    Pull {
        /// Instance name
        name: String,

        /// Branch to pull
        #[arg(short, long)]
        branch: Option<String>,
    },

    /// Show instance status
    Status {
        /// Instance name (optional if linked)
        name: Option<String>,
    },

    /// List managed instances
    List {
        /// Show all instances including terminated
        #[arg(short, long)]
        all: bool,
    },

    /// Manage EC2 profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },

    /// Configure CLI and check prerequisites
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },

    /// View cloud-init logs from instance
    Logs {
        /// Instance name
        name: String,

        /// Follow log output
        #[arg(short, long)]
        follow: bool,
    },
}

#[derive(Subcommand)]
enum ProfileCommands {
    /// List available profiles
    List,

    /// Show profile details
    Show {
        /// Profile name
        name: String,
    },

    /// Validate a profile
    Validate {
        /// Profile name
        name: String,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Initialize configuration and check prerequisites
    Init,

    /// Show current configuration
    Show,

    /// Manage custom resource tags
    Tags {
        #[command(subcommand)]
        command: TagsCommands,
    },
}

#[derive(Subcommand)]
enum TagsCommands {
    /// Set a custom tag (applied to all AWS resources)
    Set {
        /// Tag key (e.g., Username)
        key: String,
        /// Tag value
        value: String,
    },

    /// List all configured tags
    List,

    /// Remove a custom tag
    Remove {
        /// Tag key to remove
        key: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Up { profile, name, link } => {
            cli::commands::up::execute(profile, name, link).await?;
            Ok(())
        }
        Commands::Destroy { name, force } => {
            cli::commands::destroy::execute(name, force).await?;
            Ok(())
        }
        Commands::Ssh { name, command } => {
            cli::commands::ssh::execute(name, command)?;
            Ok(())
        }
        Commands::Scp {
            name,
            src,
            dest,
            recursive,
        } => {
            cli::commands::scp::execute(name, src, dest, recursive)?;
            Ok(())
        }
        Commands::Push { name, branch } => {
            cli::commands::push::execute(name, branch)?;
            Ok(())
        }
        Commands::Pull { name, branch } => {
            cli::commands::pull::execute(name, branch)?;
            Ok(())
        }
        Commands::Status { name } => {
            cli::commands::status::execute(name).await?;
            Ok(())
        }
        Commands::List { all } => {
            cli::commands::list::execute(all)?;
            Ok(())
        }
        Commands::Profile { command } => match command {
            ProfileCommands::List => {
                let loader = ProfileLoader::new();
                let profiles = loader.list()?;

                if profiles.is_empty() {
                    println!("No profiles found.");
                } else {
                    println!("{:<20} {:<10} PATH", "NAME", "SOURCE");
                    println!("{}", "-".repeat(60));
                    for info in profiles {
                        let path_str = info
                            .path
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "-".to_string());
                        println!("{:<20} {:<10} {}", info.name, info.source, path_str);
                    }
                }
                Ok(())
            }
            ProfileCommands::Show { name } => {
                let loader = ProfileLoader::new();
                let profile = loader.load(&name)?;

                println!("Profile: {}", profile.name);
                println!();
                println!("Instance:");
                println!("  Type: {}", profile.instance.instance_type);
                if !profile.instance.fallback_types.is_empty() {
                    println!("  Fallback types: {:?}", profile.instance.fallback_types);
                }
                println!("  AMI: {} ({})", profile.instance.ami.ami_type, profile.instance.ami.architecture);
                if let Some(ref ami_id) = profile.instance.ami.id {
                    println!("  AMI ID: {}", ami_id);
                }
                println!();
                println!("Storage:");
                println!("  Root volume: {} GB ({})",
                    profile.instance.storage.root_volume.size_gb,
                    profile.instance.storage.root_volume.volume_type);
                println!();
                println!("Packages:");
                if !profile.packages.system.is_empty() {
                    println!("  System: {:?}", profile.packages.system);
                }
                if profile.packages.rust.enabled {
                    println!("  Rust: {} ({:?})",
                        profile.packages.rust.channel,
                        profile.packages.rust.components);
                }
                if !profile.packages.cargo.is_empty() {
                    println!("  Cargo: {:?}", profile.packages.cargo);
                }
                if !profile.environment.is_empty() {
                    println!();
                    println!("Environment:");
                    for (key, value) in &profile.environment {
                        println!("  {}={}", key, value);
                    }
                }
                Ok(())
            }
            ProfileCommands::Validate { name } => {
                let loader = ProfileLoader::new();
                let profile = loader.load(&name)?;

                match profile.validate() {
                    Ok(()) => {
                        println!("Profile '{}' is valid.", name);
                        Ok(())
                    }
                    Err(e) => {
                        println!("Profile '{}' validation failed: {}", name, e);
                        Err(e.into())
                    }
                }
            }
        },
        Commands::Config { command } => match command {
            ConfigCommands::Init => {
                cli::commands::config::init().await?;
                Ok(())
            }
            ConfigCommands::Show => {
                cli::commands::config::show()?;
                Ok(())
            }
            ConfigCommands::Tags { command } => match command {
                TagsCommands::Set { key, value } => {
                    cli::commands::config::tags_set(&key, &value)?;
                    Ok(())
                }
                TagsCommands::List => {
                    cli::commands::config::tags_list()?;
                    Ok(())
                }
                TagsCommands::Remove { key } => {
                    cli::commands::config::tags_remove(&key)?;
                    Ok(())
                }
            },
        },
        Commands::Logs { name, follow } => {
            cli::commands::logs::execute(name, follow)?;
            Ok(())
        }
    }
}
