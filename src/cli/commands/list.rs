use crate::state::list_instances;
use crate::Result;

pub fn execute(_all: bool) -> Result<()> {
    let instances = list_instances()?;

    if instances.is_empty() {
        println!("No managed instances found.");
        println!();
        println!("Use 'ec2-cli up' to launch a new instance.");
        return Ok(());
    }

    println!(
        "{:<20} {:<20} {:<15} {:<20}",
        "NAME", "INSTANCE ID", "REGION", "CREATED"
    );
    println!("{}", "-".repeat(75));

    for (name, state) in &instances {
        println!(
            "{:<20} {:<20} {:<15} {:<20}",
            name,
            state.instance_id,
            state.region,
            state.created_at.format("%Y-%m-%d %H:%M")
        );
    }

    println!();
    println!("Total: {} instance(s)", instances.len());

    Ok(())
}
