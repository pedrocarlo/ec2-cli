pub mod config;
pub mod vcs;

pub use config::{find_git_user_config, GitUserConfig};
// Re-export VCS abstraction types for public API
#[allow(unused_imports)]
pub use vcs::{detect_vcs, Git, Jj, PullOptions, PushOptions, Vcs, VcsType};
