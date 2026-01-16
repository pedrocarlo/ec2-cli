use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::Filter;
use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_iam::Client as IamClient;
use aws_sdk_ssm::Client as SsmClient;
use aws_sdk_sts::Client as StsClient;

use crate::config::Settings;
use crate::{Ec2CliError, Result};

/// FNV-1a hash algorithm for stable hashing across Rust versions.
/// Unlike DefaultHasher, FNV-1a produces consistent results regardless
/// of Rust version or compilation target.
fn fnv1a_hash(data: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in data.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Generate a short hash based on machine hostname.
/// Used to create unique AWS resource names per machine.
pub fn machine_hash() -> String {
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    format!("{:08x}", fnv1a_hash(&hostname) & 0xFFFFFFFF)
}

/// AWS client wrapper holding all service clients
#[derive(Clone)]
pub struct AwsClients {
    pub ec2: Ec2Client,
    pub ssm: SsmClient,
    pub iam: IamClient,
    pub region: String,
    pub account_id: String,
}

impl AwsClients {
    /// Create new AWS clients, using region from settings if configured
    pub async fn new() -> Result<Self> {
        // Check if settings has a region override
        if let Ok(settings) = Settings::load() {
            if let Some(ref region) = settings.region {
                return Self::with_region(region).await;
            }
        }

        // Fall back to default configuration
        Self::new_without_settings().await
    }

    /// Create new AWS clients from default configuration (ignoring settings)
    /// Used during config init to get the AWS default region
    pub async fn new_without_settings() -> Result<Self> {
        let config = aws_config::defaults(BehaviorVersion::latest())
            .load()
            .await;

        let region = config
            .region()
            .map(|r| r.to_string())
            .ok_or(Ec2CliError::AwsCredentials)?;

        let ec2 = Ec2Client::new(&config);
        let ssm = SsmClient::new(&config);
        let iam = IamClient::new(&config);
        let sts = StsClient::new(&config);

        // Verify credentials by getting caller identity
        let identity = sts
            .get_caller_identity()
            .send()
            .await
            .map_err(|_| Ec2CliError::AwsCredentials)?;

        let account_id = identity
            .account()
            .ok_or(Ec2CliError::AwsCredentials)?
            .to_string();

        Ok(Self {
            ec2,
            ssm,
            iam,
            region,
            account_id,
        })
    }

    /// Create new AWS clients with a specific region
    pub async fn with_region(region: &str) -> Result<Self> {
        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .load()
            .await;

        let ec2 = Ec2Client::new(&config);
        let ssm = SsmClient::new(&config);
        let iam = IamClient::new(&config);
        let sts = StsClient::new(&config);

        // Verify credentials
        let identity = sts
            .get_caller_identity()
            .send()
            .await
            .map_err(|_| Ec2CliError::AwsCredentials)?;

        let account_id = identity
            .account()
            .ok_or(Ec2CliError::AwsCredentials)?
            .to_string();

        Ok(Self {
            ec2,
            ssm,
            iam,
            region: region.to_string(),
            account_id,
        })
    }
}

/// Tag used to identify resources managed by ec2-cli
pub const MANAGED_TAG_KEY: &str = "ec2-cli:managed";
pub const MANAGED_TAG_VALUE: &str = "true";

/// Tag used to store the ec2-cli instance name
pub const NAME_TAG_KEY: &str = "ec2-cli:name";

/// Standard Name tag
pub const AWS_NAME_TAG: &str = "Name";

/// Hardcoded deployment identifier tag
pub const DEPLOYMENT_TAG_KEY: &str = "deployment";
pub const DEPLOYMENT_TAG_VALUE: &str = "ec2-cli";

/// Create standard tags for a resource, including custom tags from settings
pub fn create_tags(name: &str, custom_tags: &std::collections::HashMap<String, String>) -> Vec<aws_sdk_ec2::types::Tag> {
    let mut tags = vec![
        aws_sdk_ec2::types::Tag::builder()
            .key(MANAGED_TAG_KEY)
            .value(MANAGED_TAG_VALUE)
            .build(),
        aws_sdk_ec2::types::Tag::builder()
            .key(NAME_TAG_KEY)
            .value(name)
            .build(),
        aws_sdk_ec2::types::Tag::builder()
            .key(AWS_NAME_TAG)
            .value(format!("ec2-cli-{}", name))
            .build(),
        aws_sdk_ec2::types::Tag::builder()
            .key(DEPLOYMENT_TAG_KEY)
            .value(DEPLOYMENT_TAG_VALUE)
            .build(),
    ];

    // Add custom tags from settings
    for (key, value) in custom_tags {
        tags.push(
            aws_sdk_ec2::types::Tag::builder()
                .key(key)
                .value(value)
                .build(),
        );
    }

    tags
}

/// Get the default VPC ID for the current region
pub async fn get_default_vpc(clients: &AwsClients) -> Result<String> {
    let vpcs = clients
        .ec2
        .describe_vpcs()
        .filters(
            Filter::builder()
                .name("is-default")
                .values("true")
                .build(),
        )
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    vpcs.vpcs()
        .first()
        .and_then(|v| v.vpc_id())
        .map(String::from)
        .ok_or(Ec2CliError::NoDefaultVpc)
}
