// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
mod apply;
mod baseline;
mod branch;
mod errors;
mod fsmonitor;
mod git_process;
mod info;
mod operations;
mod platform;
mod status;

/// Git configuration that rejects implicitly discovered bare repositories while
/// preserving repositories selected explicitly through `GIT_DIR` or `--git-dir`.
pub const SAFE_BARE_REPOSITORY_CONFIG: &str = "safe.bareRepository=explicit";

pub use apply::ApplyGitRequest;
pub use apply::ApplyGitResult;
pub use apply::apply_git_patch;
pub use apply::extract_paths_from_patch;
pub use apply::parse_git_apply_output;
pub use apply::stage_paths;
pub use baseline::GitBaselineChange;
pub use baseline::GitBaselineChangeStatus;
pub use baseline::GitBaselineDiff;
pub use baseline::diff_since_latest_init;
pub use baseline::ensure_git_baseline_repository;
pub use baseline::reset_git_repository;
pub use branch::merge_base_with_head;
pub use atlas_engine_protocol::protocol::GitSha;
pub use errors::GitToolingError;
pub use fsmonitor::FsmonitorOverride;
pub use fsmonitor::FsmonitorProbeRunner;
pub use fsmonitor::detect_fsmonitor_override;
pub use info::CommitLogEntry;
pub use info::GitDiffToRemote;
pub use info::GitInfo;
pub use info::canonicalize_git_remote_url;
pub use info::collect_git_info;
pub use info::current_branch_name;
pub use info::default_branch_name;
pub use info::get_git_remote_urls;
pub use info::get_git_remote_urls_assume_git_repo;
pub use info::get_git_repo_root;
pub use info::get_head_commit_hash;
pub use info::git_diff_to_remote;
pub use info::local_git_branches;
pub use info::recent_commits;
pub use info::resolve_root_git_project_for_trust;
pub use platform::create_symlink;
pub use status::get_has_changes_in_repo;

/// Atlas: Windows `CREATE_NO_WINDOW`. A child of the GUI host whose stdio is
/// piped must not be given a console window of its own; without this every
/// `git` the engine ran flashed one on the user's screen.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Atlas: apply [`CREATE_NO_WINDOW`] to a `std` command. No-op off Windows.
pub fn no_console_window(command: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}
