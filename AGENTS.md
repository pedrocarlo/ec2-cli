# ec2-cli Agent Guide

Ephemeral EC2 Development Environment Manager - a Rust CLI tool for launching and managing temporary EC2 instances for
remote development.

## Build & Test Commands

```bash
cargo build              # Build debug version
cargo build --release    # Build release version
cargo run -- <command>   # Run CLI with arguments
cargo clippy             # Run linter
cargo fmt                # Format code
cargo test               # Run tests (if any)
```

## Project Architecture

```
src/
├── main.rs              # CLI entry point, command parsing with clap
├── error.rs             # Error types (Ec2CliError, Result alias)
├── aws/
│   ├── mod.rs           # AWS module exports
│   ├── client.rs        # AWS SDK client initialization
│   ├── infrastructure.rs # VPC, security groups, IAM setup
│   └── ec2/
│       ├── mod.rs
│       └── instance.rs  # EC2 instance operations
├── cli/
│   ├── mod.rs
│   └── commands/        # Command implementations
│       ├── mod.rs
│       ├── up.rs        # Launch instance
│       ├── destroy.rs   # Terminate instance
│       ├── ssh.rs       # SSH via SSM
│       ├── scp.rs       # File copy via SSM
│       ├── push.rs      # Git push to instance
│       ├── pull.rs      # Git pull from instance
│       ├── status.rs    # Show instance status
│       ├── list.rs      # List instances
│       ├── logs.rs      # View cloud-init logs
│       └── config.rs    # Configuration management
├── config/
│   ├── mod.rs
│   └── settings.rs      # Config file handling
├── git/
│   ├── mod.rs
│   ├── operations.rs    # Git operations
│   └── remote.rs        # Remote git management
├── profile/
│   ├── mod.rs
│   ├── loader.rs        # Profile loading from files
│   └── schema.rs        # Profile JSON5 schema
├── state/
│   ├── mod.rs
│   └── local.rs         # Local state persistence
└── user_data/
    ├── mod.rs
    └── generator.rs     # EC2 user-data script generation
```

## Key Dependencies

- **clap**: CLI argument parsing with derive macros
- **tokio**: Async runtime (full features)
- **aws-sdk-***: AWS SDK for EC2, SSM, IAM, STS
- **serde/serde_json/json5**: Serialization, JSON5 for profiles
- **git2**: Git operations via libgit2
- **thiserror/anyhow**: Error handling
- **indicatif/dialoguer/console**: Terminal UI

## Code Patterns

### Error Handling

- Custom `Ec2CliError` enum in `src/error.rs` using `thiserror`
- `Result<T>` alias for `Result<T, Ec2CliError>`
- Use `anyhow::Result` at main() for top-level errors

### Async

- Commands that call AWS APIs are async (`up`, `destroy`, `status`, `config init`)
- Commands that shell out to external tools are sync (`ssh`, `scp`, `push`, `pull`, `logs`)

### AWS Client

- Clients initialized from default config with region
- All resources tagged with `ec2-cli:managed=true` and custom tags

### Profiles

- JSON5 format for readability
- Loaded from `~/.config/ec2-cli/profiles/` or `.ec2-cli/profiles/`
- Default profile embedded in code

## Configuration Files

| Path                                | Purpose                  |
|-------------------------------------|--------------------------|
| `~/.config/ec2-cli/config.json`     | Custom tags and settings |
| `~/.config/ec2-cli/profiles/`       | Global profiles          |
| `~/.local/state/ec2-cli/state.json` | Instance state tracking  |
| `.ec2-cli/profiles/`                | Project-local profiles   |
| `.ec2-cli/instance`                 | Linked instance name     |

## Adding a New Command

1. Add variant to `Commands` enum in `src/main.rs`
2. Create `src/cli/commands/<name>.rs` with `pub fn/async fn execute(...)`
3. Export from `src/cli/commands/mod.rs`
4. Add match arm in `main()` to call the command

## Security Considerations

- Instances run in private VPC with no public IP
- Access via SSM Session Manager only
- IMDSv2 required (SSRF protection)
- EBS volumes encrypted by default
- Never store AWS credentials in state files
