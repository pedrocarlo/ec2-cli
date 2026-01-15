use aws_config::BehaviorVersion;
use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_iam::Client as IamClient;
use aws_sdk_ssm::Client as SsmClient;
use aws_sdk_sts::Client as StsClient;

use crate::{Ec2CliError, Result};

/// AWS client wrapper holding all service clients
#[derive(Clone)]
pub struct AwsClients {
    pub ec2: Ec2Client,
    pub ssm: SsmClient,
    pub iam: IamClient,
    pub sts: StsClient,
    pub region: String,
    pub account_id: String,
}

impl AwsClients {
    /// Create new AWS clients from default configuration
    pub async fn new() -> Result<Self> {
        let config = aws_config::defaults(BehaviorVersion::latest())
            .load()
            .await;

        let region = config
            .region()
            .map(|r| r.to_string())
            .ok_or_else(|| Ec2CliError::AwsCredentials)?;

        let ec2 = Ec2Client::new(&config);
        let ssm = SsmClient::new(&config);
        let iam = IamClient::new(&config);
        let sts = StsClient::new(&config);

        // Verify credentials by getting caller identity
        let identity = sts
            .get_caller_identity()
            .send()
            .await
            .map_err(|e| Ec2CliError::AwsCredentials)?;

        let account_id = identity
            .account()
            .ok_or_else(|| Ec2CliError::AwsCredentials)?
            .to_string();

        Ok(Self {
            ec2,
            ssm,
            iam,
            sts,
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
            .map_err(|e| Ec2CliError::AwsCredentials)?;

        let account_id = identity
            .account()
            .ok_or_else(|| Ec2CliError::AwsCredentials)?
            .to_string();

        Ok(Self {
            ec2,
            ssm,
            iam,
            sts,
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

/// Create standard tags for a resource
pub fn create_tags(name: &str) -> Vec<aws_sdk_ec2::types::Tag> {
    vec![
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
    ]
}
