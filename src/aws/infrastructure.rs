use aws_sdk_ec2::types::{
    Filter, IpPermission, IpRange, SecurityGroup, Subnet, Tag, Vpc, VpcEndpoint,
};

use crate::{Ec2CliError, Result};

use super::client::{create_tags, AwsClients, MANAGED_TAG_KEY, MANAGED_TAG_VALUE};

const VPC_CIDR: &str = "10.0.0.0/16";
const SUBNET_CIDR: &str = "10.0.1.0/24";

/// Infrastructure resources for ec2-cli
#[derive(Debug, Clone)]
pub struct Infrastructure {
    pub vpc_id: String,
    pub subnet_id: String,
    pub security_group_id: String,
    pub instance_profile_arn: String,
    pub instance_profile_name: String,
}

impl Infrastructure {
    /// Get or create infrastructure for ec2-cli
    pub async fn get_or_create(clients: &AwsClients) -> Result<Self> {
        // Check for existing infrastructure
        if let Some(infra) = Self::find_existing(clients).await? {
            return Ok(infra);
        }

        // Create new infrastructure
        Self::create_new(clients).await
    }

    /// Find existing ec2-cli infrastructure
    async fn find_existing(clients: &AwsClients) -> Result<Option<Self>> {
        let filter = Filter::builder()
            .name(format!("tag:{}", MANAGED_TAG_KEY))
            .values(MANAGED_TAG_VALUE)
            .build();

        // Find VPC
        let vpcs = clients
            .ec2
            .describe_vpcs()
            .filters(filter.clone())
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        let vpc = match vpcs.vpcs().first() {
            Some(v) => v,
            None => return Ok(None),
        };

        let vpc_id = vpc.vpc_id().unwrap().to_string();

        // Find subnet
        let subnets = clients
            .ec2
            .describe_subnets()
            .filters(filter.clone())
            .filters(
                Filter::builder()
                    .name("vpc-id")
                    .values(&vpc_id)
                    .build(),
            )
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        let subnet_id = match subnets.subnets().first() {
            Some(s) => s.subnet_id().unwrap().to_string(),
            None => return Ok(None),
        };

        // Find security group
        let sgs = clients
            .ec2
            .describe_security_groups()
            .filters(filter.clone())
            .filters(
                Filter::builder()
                    .name("vpc-id")
                    .values(&vpc_id)
                    .build(),
            )
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        let security_group_id = match sgs.security_groups().first() {
            Some(sg) => sg.group_id().unwrap().to_string(),
            None => return Ok(None),
        };

        // Find instance profile
        let profile_name = "ec2-cli-instance-profile";
        let profile = clients
            .iam
            .get_instance_profile()
            .instance_profile_name(profile_name)
            .send()
            .await;

        let (instance_profile_arn, instance_profile_name) = match profile {
            Ok(p) => {
                let ip = p.instance_profile().unwrap();
                (
                    ip.arn().to_string(),
                    ip.instance_profile_name().to_string(),
                )
            }
            Err(_) => return Ok(None),
        };

        Ok(Some(Self {
            vpc_id,
            subnet_id,
            security_group_id,
            instance_profile_arn,
            instance_profile_name,
        }))
    }

    /// Create new infrastructure
    async fn create_new(clients: &AwsClients) -> Result<Self> {
        println!("Creating ec2-cli infrastructure...");

        // Create VPC
        println!("  Creating VPC...");
        let vpc = clients
            .ec2
            .create_vpc()
            .cidr_block(VPC_CIDR)
            .tag_specifications(
                aws_sdk_ec2::types::TagSpecification::builder()
                    .resource_type(aws_sdk_ec2::types::ResourceType::Vpc)
                    .set_tags(Some(create_tags("infrastructure")))
                    .build(),
            )
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        let vpc_id = vpc.vpc().unwrap().vpc_id().unwrap().to_string();

        // Enable DNS hostnames
        clients
            .ec2
            .modify_vpc_attribute()
            .vpc_id(&vpc_id)
            .enable_dns_hostnames(
                aws_sdk_ec2::types::AttributeBooleanValue::builder()
                    .value(true)
                    .build(),
            )
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        // Create subnet
        println!("  Creating subnet...");
        let subnet = clients
            .ec2
            .create_subnet()
            .vpc_id(&vpc_id)
            .cidr_block(SUBNET_CIDR)
            .tag_specifications(
                aws_sdk_ec2::types::TagSpecification::builder()
                    .resource_type(aws_sdk_ec2::types::ResourceType::Subnet)
                    .set_tags(Some(create_tags("infrastructure")))
                    .build(),
            )
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        let subnet_id = subnet.subnet().unwrap().subnet_id().unwrap().to_string();

        // Create security group
        println!("  Creating security group...");
        let sg = clients
            .ec2
            .create_security_group()
            .group_name("ec2-cli-sg")
            .description("Security group for ec2-cli instances")
            .vpc_id(&vpc_id)
            .tag_specifications(
                aws_sdk_ec2::types::TagSpecification::builder()
                    .resource_type(aws_sdk_ec2::types::ResourceType::SecurityGroup)
                    .set_tags(Some(create_tags("infrastructure")))
                    .build(),
            )
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        let security_group_id = sg.group_id().unwrap().to_string();

        // Add egress rule for HTTPS (for VPC endpoints)
        clients
            .ec2
            .authorize_security_group_egress()
            .group_id(&security_group_id)
            .ip_permissions(
                IpPermission::builder()
                    .ip_protocol("tcp")
                    .from_port(443)
                    .to_port(443)
                    .ip_ranges(IpRange::builder().cidr_ip(VPC_CIDR).build())
                    .build(),
            )
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        // Revoke default egress rule (0.0.0.0/0)
        let _ = clients
            .ec2
            .revoke_security_group_egress()
            .group_id(&security_group_id)
            .ip_permissions(
                IpPermission::builder()
                    .ip_protocol("-1")
                    .ip_ranges(IpRange::builder().cidr_ip("0.0.0.0/0").build())
                    .build(),
            )
            .send()
            .await;

        // Create VPC endpoints
        println!("  Creating VPC endpoints...");
        create_vpc_endpoints(clients, &vpc_id, &subnet_id, &security_group_id).await?;

        // Create IAM role and instance profile
        println!("  Creating IAM role and instance profile...");
        let (instance_profile_arn, instance_profile_name) =
            create_iam_resources(clients).await?;

        println!("Infrastructure created successfully.");

        Ok(Self {
            vpc_id,
            subnet_id,
            security_group_id,
            instance_profile_arn,
            instance_profile_name,
        })
    }
}

/// Create VPC endpoints for SSM
async fn create_vpc_endpoints(
    clients: &AwsClients,
    vpc_id: &str,
    subnet_id: &str,
    security_group_id: &str,
) -> Result<()> {
    let endpoints = [
        "com.amazonaws.{region}.ssm",
        "com.amazonaws.{region}.ssmmessages",
        "com.amazonaws.{region}.ec2messages",
    ];

    for endpoint_template in endpoints {
        let service_name = endpoint_template.replace("{region}", &clients.region);

        // Check if endpoint already exists
        let existing = clients
            .ec2
            .describe_vpc_endpoints()
            .filters(
                Filter::builder()
                    .name("service-name")
                    .values(&service_name)
                    .build(),
            )
            .filters(Filter::builder().name("vpc-id").values(vpc_id).build())
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        if !existing.vpc_endpoints().is_empty() {
            continue;
        }

        clients
            .ec2
            .create_vpc_endpoint()
            .vpc_id(vpc_id)
            .service_name(&service_name)
            .vpc_endpoint_type(aws_sdk_ec2::types::VpcEndpointType::Interface)
            .subnet_ids(subnet_id)
            .security_group_ids(security_group_id)
            .private_dns_enabled(true)
            .tag_specifications(
                aws_sdk_ec2::types::TagSpecification::builder()
                    .resource_type(aws_sdk_ec2::types::ResourceType::VpcEndpoint)
                    .set_tags(Some(create_tags("infrastructure")))
                    .build(),
            )
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;
    }

    // Create S3 gateway endpoint for package downloads
    let s3_service = format!("com.amazonaws.{}.s3", clients.region);

    // Get route table for the VPC
    let route_tables = clients
        .ec2
        .describe_route_tables()
        .filters(Filter::builder().name("vpc-id").values(vpc_id).build())
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    if let Some(rt) = route_tables.route_tables().first() {
        let rt_id = rt.route_table_id().unwrap();

        // Check if S3 endpoint already exists
        let existing = clients
            .ec2
            .describe_vpc_endpoints()
            .filters(
                Filter::builder()
                    .name("service-name")
                    .values(&s3_service)
                    .build(),
            )
            .filters(Filter::builder().name("vpc-id").values(vpc_id).build())
            .send()
            .await
            .map_err(Ec2CliError::ec2)?;

        if existing.vpc_endpoints().is_empty() {
            clients
                .ec2
                .create_vpc_endpoint()
                .vpc_id(vpc_id)
                .service_name(&s3_service)
                .vpc_endpoint_type(aws_sdk_ec2::types::VpcEndpointType::Gateway)
                .route_table_ids(rt_id)
                .tag_specifications(
                    aws_sdk_ec2::types::TagSpecification::builder()
                        .resource_type(aws_sdk_ec2::types::ResourceType::VpcEndpoint)
                        .set_tags(Some(create_tags("infrastructure")))
                        .build(),
                )
                .send()
                .await
                .map_err(Ec2CliError::ec2)?;
        }
    }

    Ok(())
}

/// Create IAM role and instance profile for SSM
async fn create_iam_resources(clients: &AwsClients) -> Result<(String, String)> {
    let role_name = "ec2-cli-instance-role";
    let profile_name = "ec2-cli-instance-profile";

    // Check if role already exists
    let existing_role = clients
        .iam
        .get_role()
        .role_name(role_name)
        .send()
        .await;

    if existing_role.is_err() {
        // Create the role
        let assume_role_policy = r#"{
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": {
                        "Service": "ec2.amazonaws.com"
                    },
                    "Action": "sts:AssumeRole"
                }
            ]
        }"#;

        clients
            .iam
            .create_role()
            .role_name(role_name)
            .assume_role_policy_document(assume_role_policy)
            .description("Role for ec2-cli managed instances")
            .tags(
                aws_sdk_iam::types::Tag::builder()
                    .key(MANAGED_TAG_KEY)
                    .value(MANAGED_TAG_VALUE)
                    .build()
                    .map_err(|e| Ec2CliError::Iam(e.to_string()))?,
            )
            .send()
            .await
            .map_err(Ec2CliError::iam)?;

        // Attach SSM managed policy
        clients
            .iam
            .attach_role_policy()
            .role_name(role_name)
            .policy_arn("arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore")
            .send()
            .await
            .map_err(Ec2CliError::iam)?;
    }

    // Check if instance profile exists
    let existing_profile = clients
        .iam
        .get_instance_profile()
        .instance_profile_name(profile_name)
        .send()
        .await;

    let profile_arn = match existing_profile {
        Ok(p) => p
            .instance_profile()
            .unwrap()
            .arn()
            .to_string(),
        Err(_) => {
            // Create instance profile
            let profile = clients
                .iam
                .create_instance_profile()
                .instance_profile_name(profile_name)
                .tags(
                    aws_sdk_iam::types::Tag::builder()
                        .key(MANAGED_TAG_KEY)
                        .value(MANAGED_TAG_VALUE)
                        .build()
                        .map_err(|e| Ec2CliError::Iam(e.to_string()))?,
                )
                .send()
                .await
                .map_err(Ec2CliError::iam)?;

            // Add role to instance profile
            clients
                .iam
                .add_role_to_instance_profile()
                .instance_profile_name(profile_name)
                .role_name(role_name)
                .send()
                .await
                .map_err(Ec2CliError::iam)?;

            // Wait a bit for propagation
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            profile
                .instance_profile()
                .unwrap()
                .arn()
                .to_string()
        }
    };

    Ok((profile_arn, profile_name.to_string()))
}
