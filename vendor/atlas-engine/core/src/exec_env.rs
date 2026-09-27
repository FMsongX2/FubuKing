// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
pub use atlas_engine_apply_patch::ATLAS_AGENT_APPLY_PATCH_PRESERVE_LINE_ENDINGS_ENV_VAR;
use atlas_engine_features::Feature;
use atlas_engine_features::Features;
use atlas_engine_protocol::SessionId;
use atlas_engine_protocol::ThreadId;
#[cfg(test)]
use atlas_engine_protocol::config_types::EnvironmentVariablePattern;
use atlas_engine_protocol::config_types::ShellEnvironmentPolicy;
use atlas_engine_protocol::models::ActivePermissionProfile;
use atlas_engine_protocol::shell_environment;
use std::collections::HashMap;

pub use atlas_engine_protocol::shell_environment::ATLAS_AGENT_SESSION_ID_ENV_VAR;
pub use atlas_engine_protocol::shell_environment::ATLAS_AGENT_THREAD_ID_ENV_VAR;

/// Informational name of the active permission profile. Child processes can
/// overwrite this value, so it must not be treated as proof of enforcement.
pub const ATLAS_AGENT_PERMISSION_PROFILE_ENV_VAR: &str = "ATLAS_AGENT_PERMISSION_PROFILE";

/// Construct an environment map based on the rules in the specified policy. The
/// resulting map can be passed directly to `Command::envs()` after calling
/// `env_clear()` to ensure no unintended variables are leaked to the spawned
/// process.
///
/// The derivation follows the algorithm documented in the struct-level comment
/// for [`ShellEnvironmentPolicy`].
///
/// `ATLAS_AGENT_THREAD_ID` is injected when a thread id is provided, even when
/// `include_only` is set.
pub fn create_env(
    policy: &ShellEnvironmentPolicy,
    thread_id: Option<ThreadId>,
) -> HashMap<String, String> {
    let thread_id = thread_id.map(|thread_id| thread_id.to_string());
    shell_environment::create_env(policy, thread_id.as_deref())
}

/// Exposes the shared root-session identity to model-reachable shell commands.
pub(crate) fn inject_session_id_env(env: &mut HashMap<String, String>, session_id: SessionId) {
    env.insert(ATLAS_AGENT_SESSION_ID_ENV_VAR.to_string(), session_id.to_string());
}

/// Injects the selected named permission profile into a shell tool's environment.
///
/// This is applied after the shell environment policy so the runtime-selected
/// profile wins over inherited or configured values.
pub(crate) fn inject_permission_profile_env(
    env: &mut HashMap<String, String>,
    active_permission_profile: Option<&ActivePermissionProfile>,
) {
    if cfg!(windows) {
        env.retain(|key, _| !key.eq_ignore_ascii_case(ATLAS_AGENT_PERMISSION_PROFILE_ENV_VAR));
    } else {
        env.remove(ATLAS_AGENT_PERMISSION_PROFILE_ENV_VAR);
    }
    if let Some(active_permission_profile) = active_permission_profile {
        env.insert(
            ATLAS_AGENT_PERMISSION_PROFILE_ENV_VAR.to_string(),
            active_permission_profile.id.clone(),
        );
    }
}

/// Carries the configured apply-patch line-ending rollout state into child
/// processes.
///
/// Apply this after inherited or client-provided environment overrides so the
/// active feature configuration remains authoritative. The in-process
/// apply-patch path reads the feature directly.
pub fn inject_apply_patch_env(env: &mut HashMap<String, String>, features: &Features) {
    env.retain(|key, _| !key.eq_ignore_ascii_case(ATLAS_AGENT_APPLY_PATCH_PRESERVE_LINE_ENDINGS_ENV_VAR));
    if features.enabled(Feature::ApplyPatchPreserveLineEndings) {
        env.insert(
            ATLAS_AGENT_APPLY_PATCH_PRESERVE_LINE_ENDINGS_ENV_VAR.to_string(),
            "1".to_string(),
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
fn create_env_from_vars<I>(
    vars: I,
    policy: &ShellEnvironmentPolicy,
    thread_id: Option<ThreadId>,
) -> HashMap<String, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    let thread_id = thread_id.map(|thread_id| thread_id.to_string());
    shell_environment::create_env_from_vars(vars, policy, thread_id.as_deref())
}

#[cfg(test)]
fn populate_env<I>(
    vars: I,
    policy: &ShellEnvironmentPolicy,
    thread_id: Option<ThreadId>,
) -> HashMap<String, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    let thread_id = thread_id.map(|thread_id| thread_id.to_string());
    shell_environment::populate_env(vars, policy, thread_id.as_deref())
}

#[cfg(test)]
#[path = "exec_env_tests.rs"]
mod tests;
