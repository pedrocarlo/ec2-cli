//! Shell completion generation and custom completers for dynamic values.

use clap::ValueEnum;
use clap_complete::engine::{CompletionCandidate, ValueCompleter};

use crate::profile::ProfileLoader;
use crate::state::State;

/// Shell types for completion script generation.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellType {
    Bash,
    Zsh,
    Fish,
}

/// Completer for instance names from local state.
#[derive(Clone, Default)]
pub struct InstanceCompleter;

impl ValueCompleter for InstanceCompleter {
    fn complete(&self, _current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
        let Ok(state) = State::load() else {
            return Vec::new();
        };

        state
            .instances
            .keys()
            .map(CompletionCandidate::new)
            .collect()
    }
}

/// Completer for profile names from profile loader.
#[derive(Clone, Default)]
pub struct ProfileCompleter;

impl ValueCompleter for ProfileCompleter {
    fn complete(&self, _current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
        let loader = ProfileLoader::new();
        let Ok(profiles) = loader.list() else {
            return Vec::new();
        };

        profiles
            .into_iter()
            .map(|info| CompletionCandidate::new(info.name))
            .collect()
    }
}
