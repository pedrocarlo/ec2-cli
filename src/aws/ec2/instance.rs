use aws_sdk_ec2::types::{
    BlockDeviceMapping, EbsBlockDevice, Filter, Instance, InstanceStateName,
    InstanceType as AwsInstanceType, Tag,
};

use crate::profile::Profile;
use crate::{Ec2CliError, Result};

use super::super::client::{create_tags, AwsClients, MANAGED_TAG_KEY, MANAGED_TAG_VALUE, NAME_TAG_KEY};
use super::super::infrastructure::Infrastructure;

/// Launch a new EC2 instance
pub async fn launch_instance(
    clients: &AwsClients,
    infra: &Infrastructure,
    profile: &Profile,
    name: &str,
    user_data: &str,
) -> Result<String> {
    // Look up AMI
    let ami_id = lookup_ami(clients, profile).await?;

    // Parse instance type
    let instance_type = AwsInstanceType::from(profile.instance.instance_type.as_str());

    // Create block device mapping
    let root_volume = &profile.instance.storage.root_volume;
    let mut ebs_builder = EbsBlockDevice::builder()
        .volume_size(root_volume.size_gb as i32)
        .volume_type(aws_sdk_ec2::types::VolumeType::from(
            root_volume.volume_type.as_str(),
        ))
        .delete_on_termination(true);

    if let Some(iops) = root_volume.iops {
        ebs_builder = ebs_builder.iops(iops as i32);
    }
    if let Some(throughput) = root_volume.throughput {
        ebs_builder = ebs_builder.throughput(throughput as i32);
    }

    let block_device = BlockDeviceMapping::builder()
        .device_name("/dev/xvda")
        .ebs(ebs_builder.build())
        .build();

    // Encode user data
    let user_data_encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        user_data.as_bytes(),
    );

    // Launch instance
    let run_result = clients
        .ec2
        .run_instances()
        .image_id(&ami_id)
        .instance_type(instance_type)
        .min_count(1)
        .max_count(1)
        .subnet_id(&infra.subnet_id)
        .security_group_ids(&infra.security_group_id)
        .iam_instance_profile(
            aws_sdk_ec2::types::IamInstanceProfileSpecification::builder()
                .arn(&infra.instance_profile_arn)
                .build(),
        )
        .block_device_mappings(block_device)
        .user_data(&user_data_encoded)
        .tag_specifications(
            aws_sdk_ec2::types::TagSpecification::builder()
                .resource_type(aws_sdk_ec2::types::ResourceType::Instance)
                .set_tags(Some(create_tags(name)))
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

    // Build filters based on AMI type
    let (owner, name_pattern) = match ami_config.ami_type.as_str() {
        "amazon-linux-2023" => {
            let arch = match ami_config.architecture.as_str() {
                "arm64" => "arm64",
                _ => "x86_64",
            };
            (
                "amazon",
                format!("al2023-ami-2023.*-kernel-*-{}", arch),
            )
        }
        "amazon-linux-2" => {
            let arch = match ami_config.architecture.as_str() {
                "arm64" => "arm64",
                _ => "x86_64",
            };
            (
                "amazon",
                format!("amzn2-ami-hvm-*-{}-gp2", arch),
            )
        }
        "ubuntu-22.04" => {
            let arch = match ami_config.architecture.as_str() {
                "arm64" => "arm64",
                _ => "amd64",
            };
            (
                "099720109477", // Canonical
                format!("ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-{}-server-*", arch),
            )
        }
        "ubuntu-24.04" => {
            let arch = match ami_config.architecture.as_str() {
                "arm64" => "arm64",
                _ => "amd64",
            };
            (
                "099720109477", // Canonical
                format!("ubuntu/images/hvm-ssd-gp3/ubuntu-noble-24.04-{}-server-*", arch),
            )
        }
        other => {
            return Err(Ec2CliError::ProfileValidation(format!(
                "Unknown AMI type: {}",
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

/// Get instance by name
pub async fn get_instance_by_name(
    clients: &AwsClients,
    name: &str,
) -> Result<Option<Instance>> {
    let result = clients
        .ec2
        .describe_instances()
        .filters(
            Filter::builder()
                .name(format!("tag:{}", NAME_TAG_KEY))
                .values(name)
                .build(),
        )
        .filters(
            Filter::builder()
                .name(format!("tag:{}", MANAGED_TAG_KEY))
                .values(MANAGED_TAG_VALUE)
                .build(),
        )
        .filters(
            Filter::builder()
                .name("instance-state-name")
                .values("pending")
                .values("running")
                .values("stopping")
                .values("stopped")
                .build(),
        )
        .send()
        .await
        .map_err(Ec2CliError::ec2)?;

    Ok(result
        .reservations()
        .first()
        .and_then(|r| r.instances().first())
        .cloned())
}

/// List all managed instances
pub async fn list_managed_instances(
    clients: &AwsClients,
    include_terminated: bool,
) -> Result<Vec<Instance>> {
    let mut builder = clients
        .ec2
        .describe_instances()
        .filters(
            Filter::builder()
                .name(format!("tag:{}", MANAGED_TAG_KEY))
                .values(MANAGED_TAG_VALUE)
                .build(),
        );

    if !include_terminated {
        builder = builder.filters(
            Filter::builder()
                .name("instance-state-name")
                .values("pending")
                .values("running")
                .values("stopping")
                .values("stopped")
                .build(),
        );
    }

    let result = builder.send().await.map_err(Ec2CliError::ec2)?;

    let mut instances = Vec::new();
    for reservation in result.reservations() {
        instances.extend(reservation.instances().iter().cloned());
    }

    Ok(instances)
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

/// Get tag value from instance
pub fn get_tag_value(instance: &Instance, key: &str) -> Option<String> {
    instance
        .tags()
        .iter()
        .find(|t| t.key() == Some(key))
        .and_then(|t| t.value().map(String::from))
}
