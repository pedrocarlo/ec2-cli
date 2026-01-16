use std::collections::HashMap;

use aws_sdk_ec2::types::{
    BlockDeviceMapping, EbsBlockDevice, Filter, HttpTokensState,
    InstanceMetadataEndpointState, InstanceMetadataOptionsRequest, InstanceStateName,
    InstanceType as AwsInstanceType,
};
use uuid::Uuid;

use crate::config::Settings;
use crate::profile::Profile;
use crate::{Ec2CliError, Result};

use super::super::client::{create_tags, AwsClients};
use super::super::infrastructure::Infrastructure;

/// Create a per-instance security group
pub async fn create_instance_security_group(
    clients: &AwsClients,
    vpc_id: &str,
    instance_name: &str,
    custom_tags: &HashMap<String, String>,
) -> Result<String> {
    // Generate unique suffix for security group name
    let hash = &Uuid::new_v4().to_string()[..8];
    let sg_name = format!("ec2-cli-{}-{}", instance_name, hash);

    let sg = clients
        .ec2
        .create_security_group()
        .group_name(&sg_name)
        .description(format!("Security group for ec2-cli instance {}", instance_name))
        .vpc_id(vpc_id)
        .tag_specifications(
            aws_sdk_ec2::types::TagSpecification::builder()
                .resource_type(aws_sdk_ec2::types::ResourceType::SecurityGroup)
                .set_tags(Some(create_tags(instance_name, custom_tags)))
                .build(),
        )
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    let security_group_id = sg
        .group_id()
        .ok_or_else(|| Ec2CliError::Ec2("No security group ID returned".to_string()))?
        .to_string();

    // Security group has default egress rule (0.0.0.0/0) which is needed for SSM via internet
    // No inbound rules are needed - SSM Session Manager doesn't require inbound ports

    Ok(security_group_id)
}

/// Delete a security group
pub async fn delete_security_group(clients: &AwsClients, security_group_id: &str) -> Result<()> {
    clients
        .ec2
        .delete_security_group()
        .group_id(security_group_id)
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    Ok(())
}

/// Launch a new EC2 instance
pub async fn launch_instance(
    clients: &AwsClients,
    infra: &Infrastructure,
    security_group_id: &str,
    profile: &Profile,
    name: &str,
    user_data: &str,
) -> Result<String> {
    // Load custom tags from settings
    let custom_tags = Settings::load()
        .map(|s| s.tags)
        .unwrap_or_default();

    // Look up AMI
    let ami_id = lookup_ami(clients, profile).await?;

    // Parse instance type
    let instance_type = AwsInstanceType::from(profile.instance.instance_type.as_str());

    // Create block device mapping with encryption always enabled
    let root_volume = &profile.instance.storage.root_volume;
    let mut ebs_builder = EbsBlockDevice::builder()
        .volume_size(root_volume.size_gb as i32)
        .volume_type(aws_sdk_ec2::types::VolumeType::from(
            root_volume.volume_type.as_str(),
        ))
        .delete_on_termination(true)
        .encrypted(true); // Always encrypt EBS volumes

    if let Some(iops) = root_volume.iops {
        ebs_builder = ebs_builder.iops(iops as i32);
    }
    if let Some(throughput) = root_volume.throughput {
        ebs_builder = ebs_builder.throughput(throughput as i32);
    }

    // Ubuntu AMIs use /dev/sda1 as root device (unlike Amazon Linux which uses /dev/xvda)
    let block_device = BlockDeviceMapping::builder()
        .device_name("/dev/sda1")
        .ebs(ebs_builder.build())
        .build();

    // Encode user data
    let user_data_encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        user_data.as_bytes(),
    );

    // Launch instance with IMDSv2 required (prevents SSRF credential theft)
    let run_result = clients
        .ec2
        .run_instances()
        .image_id(&ami_id)
        .instance_type(instance_type)
        .min_count(1)
        .max_count(1)
        .subnet_id(&infra.subnet_id)
        .security_group_ids(security_group_id)
        .iam_instance_profile(
            aws_sdk_ec2::types::IamInstanceProfileSpecification::builder()
                .arn(&infra.instance_profile_arn)
                .build(),
        )
        .block_device_mappings(block_device)
        .user_data(&user_data_encoded)
        .metadata_options(
            InstanceMetadataOptionsRequest::builder()
                .http_tokens(HttpTokensState::Required) // Enforce IMDSv2
                .http_put_response_hop_limit(1)
                .http_endpoint(InstanceMetadataEndpointState::Enabled)
                .build(),
        )
        .tag_specifications(
            aws_sdk_ec2::types::TagSpecification::builder()
                .resource_type(aws_sdk_ec2::types::ResourceType::Instance)
                .set_tags(Some(create_tags(name, &custom_tags)))
                .build(),
        )
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    let instance = run_result
        .instances()
        .first()
        .ok_or_else(|| Ec2CliError::Ec2("No instance returned".to_string()))?;

    let instance_id = instance
        .instance_id()
        .ok_or_else(|| Ec2CliError::Ec2("No instance ID".to_string()))?
        .to_string();

    Ok(instance_id)
}

/// Look up AMI ID based on profile configuration
pub async fn lookup_ami(clients: &AwsClients, profile: &Profile) -> Result<String> {
    // If specific AMI ID is provided, use it
    if let Some(ref ami_id) = profile.instance.ami.id {
        return Ok(ami_id.clone());
    }

    let ami_config = &profile.instance.ami;

    // Build filters based on AMI type (Ubuntu only)
    let arch = match ami_config.architecture.as_str() {
        "arm64" => "arm64",
        _ => "amd64",
    };

    let (owner, name_pattern) = match ami_config.ami_type.as_str() {
        "ubuntu-22.04" => (
            "099720109477", // Canonical
            format!("ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-{}-server-*", arch),
        ),
        "ubuntu-24.04" => (
            "099720109477", // Canonical
            format!("ubuntu/images/hvm-ssd-gp3/ubuntu-noble-24.04-{}-server-*", arch),
        ),
        other => {
            return Err(Ec2CliError::ProfileValidation(format!(
                "Unknown AMI type: {}. Supported: ubuntu-22.04, ubuntu-24.04",
                other
            )));
        }
    };

    let images = clients
        .ec2
        .describe_images()
        .owners(owner)
        .filters(
            Filter::builder()
                .name("name")
                .values(&name_pattern)
                .build(),
        )
        .filters(
            Filter::builder()
                .name("state")
                .values("available")
                .build(),
        )
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    // Sort by creation date and get the latest
    let mut images: Vec<_> = images.images().to_vec();
    images.sort_by(|a, b| {
        let a_date = a.creation_date().unwrap_or_default();
        let b_date = b.creation_date().unwrap_or_default();
        b_date.cmp(a_date) // Descending order
    });

    images
        .first()
        .and_then(|i| i.image_id().map(String::from))
        .ok_or_else(|| {
            Ec2CliError::ResourceNotFound(format!(
                "No AMI found matching {} for {}",
                ami_config.ami_type, ami_config.architecture
            ))
        })
}

/// Wait for instance to be running
pub async fn wait_for_running(
    clients: &AwsClients,
    instance_id: &str,
    timeout_secs: u64,
) -> Result<()> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    loop {
        if start.elapsed() > timeout {
            return Err(Ec2CliError::Timeout(format!(
                "Instance {} did not reach running state within {} seconds",
                instance_id, timeout_secs
            )));
        }

        let state = get_instance_state(clients, instance_id).await?;

        match state {
            InstanceStateName::Running => return Ok(()),
            InstanceStateName::Pending => {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
            other => {
                return Err(Ec2CliError::InstanceState(format!(
                    "Instance {} in unexpected state: {:?}",
                    instance_id, other
                )));
            }
        }
    }
}

/// Wait for instance to be ready (SSM agent online)
pub async fn wait_for_ssm_ready(
    clients: &AwsClients,
    instance_id: &str,
    timeout_secs: u64,
) -> Result<()> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    loop {
        if start.elapsed() > timeout {
            return Err(Ec2CliError::Timeout(format!(
                "Instance {} SSM agent did not become ready within {} seconds",
                instance_id, timeout_secs
            )));
        }

        let filter = aws_sdk_ssm::types::InstanceInformationStringFilter::builder()
            .key("InstanceIds")
            .values(instance_id)
            .build()
            .map_err(|e| Ec2CliError::Ssm(e.to_string()))?;

        let info = clients
            .ssm
            .describe_instance_information()
            .filters(filter)
            .send()
            .await
            .map_err(Ec2CliError::ssm)?;

        if let Some(instance_info) = info.instance_information_list().first() {
            if instance_info.ping_status()
                == Some(&aws_sdk_ssm::types::PingStatus::Online)
            {
                return Ok(());
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    }
}

/// Get instance state
pub async fn get_instance_state(
    clients: &AwsClients,
    instance_id: &str,
) -> Result<InstanceStateName> {
    let result = clients
        .ec2
        .describe_instances()
        .instance_ids(instance_id)
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    let instance = result
        .reservations()
        .first()
        .and_then(|r| r.instances().first())
        .ok_or_else(|| Ec2CliError::InstanceNotFound(instance_id.to_string()))?;

    instance
        .state()
        .and_then(|s| s.name().cloned())
        .ok_or_else(|| Ec2CliError::InstanceState("Unknown state".to_string()))
}

/// Terminate an instance
pub async fn terminate_instance(clients: &AwsClients, instance_id: &str) -> Result<()> {
    clients
        .ec2
        .terminate_instances()
        .instance_ids(instance_id)
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    Ok(())
}

/// Wait for instance to be terminated
pub async fn wait_for_terminated(
    clients: &AwsClients,
    instance_id: &str,
    timeout_secs: u64,
) -> Result<()> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    loop {
        if start.elapsed() > timeout {
            return Err(Ec2CliError::Timeout(format!(
                "Instance {} did not terminate within {} seconds",
                instance_id, timeout_secs
            )));
        }

        // If instance is no longer found, treat it as terminated
        let state = match get_instance_state(clients, instance_id).await {
            Ok(s) => s,
            Err(Ec2CliError::InstanceNotFound(_)) => return Ok(()),
            Err(e) => return Err(e),
        };

        match state {
            InstanceStateName::Terminated => return Ok(()),
            // Valid intermediate states during termination
            InstanceStateName::ShuttingDown
            | InstanceStateName::Stopping
            | InstanceStateName::Stopped
            | InstanceStateName::Running => {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
            other => {
                return Err(Ec2CliError::InstanceState(format!(
                    "Instance {} in unexpected state during termination: {:?}",
                    instance_id, other
                )));
            }
        }
    }
}
