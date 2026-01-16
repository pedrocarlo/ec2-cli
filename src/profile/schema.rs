use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub instance: InstanceConfig,
    #[serde(default)]
    pub packages: PackageConfig,
    #[serde(default)]
    pub environment: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    #[serde(rename = "type", default = "default_instance_type")]
    pub instance_type: String,
    #[serde(default)]
    pub fallback_types: Vec<String>,
    #[serde(default)]
    pub ami: AmiConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

impl Default for InstanceConfig {
    fn default() -> Self {
        Self {
            instance_type: default_instance_type(),
            fallback_types: vec!["t3.medium".to_string()],
            ami: AmiConfig::default(),
            storage: StorageConfig::default(),
        }
    }
}

fn default_instance_type() -> String {
    "t3.large".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmiConfig {
    #[serde(rename = "type", default = "default_ami_type")]
    pub ami_type: String,
    #[serde(default = "default_architecture")]
    pub architecture: String,
    /// Optional specific AMI ID (overrides type lookup)
    pub id: Option<String>,
}

impl Default for AmiConfig {
    fn default() -> Self {
        Self {
            ami_type: default_ami_type(),
            architecture: default_architecture(),
            id: None,
        }
    }
}

fn default_ami_type() -> String {
    "ubuntu-24.04".to_string()
}

fn default_architecture() -> String {
    "x86_64".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    #[serde(default)]
    pub root_volume: RootVolumeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootVolumeConfig {
    #[serde(default = "default_volume_size")]
    pub size_gb: u32,
    #[serde(rename = "type", default = "default_volume_type")]
    pub volume_type: String,
    #[serde(default = "default_iops")]
    pub iops: Option<u32>,
    #[serde(default)]
    pub throughput: Option<u32>,
}

impl Default for RootVolumeConfig {
    fn default() -> Self {
        Self {
            size_gb: default_volume_size(),
            volume_type: default_volume_type(),
            iops: Some(default_iops().unwrap()),
            throughput: Some(125),
        }
    }
}

fn default_volume_size() -> u32 {
    30
}

fn default_volume_type() -> String {
    "gp3".to_string()
}

fn default_iops() -> Option<u32> {
    Some(3000)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageConfig {
    #[serde(default)]
    pub system: Vec<String>,
    #[serde(default)]
    pub rust: RustConfig,
    #[serde(default)]
    pub cargo: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_rust_channel")]
    pub channel: String,
    #[serde(default)]
    pub components: Vec<String>,
}

impl Default for RustConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            channel: default_rust_channel(),
            components: vec!["rustfmt".to_string(), "clippy".to_string()],
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_rust_channel() -> String {
    "stable".to_string()
}

impl Profile {
    pub fn default_profile() -> Self {
        Self {
            name: "default".to_string(),
            instance: InstanceConfig::default(),
            packages: PackageConfig {
                system: vec![
                    "build-essential".to_string(),
                    "libssl-dev".to_string(),
                    "pkg-config".to_string(),
                    "git".to_string(),
                ],
                rust: RustConfig::default(),
                cargo: vec![],
            },
            environment: HashMap::new(),
        }
    }

    pub fn validate(&self) -> crate::Result<()> {
        if self.name.is_empty() {
            return Err(crate::Ec2CliError::ProfileValidation(
                "Profile name cannot be empty".to_string(),
            ));
        }

        if self.instance.instance_type.is_empty() {
            return Err(crate::Ec2CliError::ProfileValidation(
                "Instance type cannot be empty".to_string(),
            ));
        }

        if self.instance.storage.root_volume.size_gb < 8 {
            return Err(crate::Ec2CliError::ProfileValidation(
                "Root volume size must be at least 8 GB".to_string(),
            ));
        }

        if self.instance.storage.root_volume.size_gb > 16384 {
            return Err(crate::Ec2CliError::ProfileValidation(
                "Root volume size cannot exceed 16384 GB".to_string(),
            ));
        }

        let valid_volume_types = ["gp2", "gp3", "io1", "io2", "st1", "sc1"];
        if !valid_volume_types.contains(&self.instance.storage.root_volume.volume_type.as_str()) {
            return Err(crate::Ec2CliError::ProfileValidation(format!(
                "Invalid volume type: {}. Valid types: {:?}",
                self.instance.storage.root_volume.volume_type, valid_volume_types
            )));
        }

        let valid_architectures = ["x86_64", "arm64"];
        if !valid_architectures.contains(&self.instance.ami.architecture.as_str()) {
            return Err(crate::Ec2CliError::ProfileValidation(format!(
                "Invalid architecture: {}. Valid: {:?}",
                self.instance.ami.architecture, valid_architectures
            )));
        }

        let valid_ami_types = ["ubuntu-22.04", "ubuntu-24.04"];
        if self.instance.ami.id.is_none()
            && !valid_ami_types.contains(&self.instance.ami.ami_type.as_str())
        {
            return Err(crate::Ec2CliError::ProfileValidation(format!(
                "Invalid AMI type: {}. Valid: {:?}",
                self.instance.ami.ami_type, valid_ami_types
            )));
        }

        let valid_rust_channels = ["stable", "beta", "nightly"];
        if self.packages.rust.enabled
            && !valid_rust_channels.contains(&self.packages.rust.channel.as_str())
        {
            return Err(crate::Ec2CliError::ProfileValidation(format!(
                "Invalid Rust channel: {}. Valid: {:?}",
                self.packages.rust.channel, valid_rust_channels
            )));
        }

        Ok(())
    }
}
