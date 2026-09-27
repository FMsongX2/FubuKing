// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
/*
Module: sandboxing

Core-owned adapter types for exec/runtime plumbing. Policy selection and
command transformation live in the atlas-engine-sandboxing crate; this module keeps
the exec-only metadata and translates transformed sandbox commands back into
ExecRequest for execution.
*/

use crate::exec::ExecCapturePolicy;
use crate::exec::ExecExpiration;
use crate::exec::StdoutStream;
use crate::exec::execute_exec_request;
#[cfg(target_os = "macos")]
use crate::spawn::ATLAS_AGENT_SANDBOX_ENV_VAR;
use crate::spawn::ATLAS_AGENT_SANDBOX_NETWORK_DISABLED_ENV_VAR;
use atlas_engine_file_system::FileSystemSandboxContext;
use atlas_engine_network_proxy::ManagedNetworkSandboxContext;
use atlas_engine_network_proxy::NetworkProxy;
use atlas_engine_network_proxy::RemoteNetworkProxyLaunchConfig;
use atlas_engine_protocol::config_types::WindowsSandboxLevel;
use atlas_engine_protocol::exec_output::ExecToolCallOutput;
use atlas_engine_protocol::models::PermissionProfile;
pub use atlas_engine_protocol::models::SandboxPermissions;
use atlas_engine_sandboxing::SandboxExecRequest;
use atlas_engine_sandboxing::SandboxType;
use atlas_engine_sandboxing::WindowsSandboxFilesystemOverrides;
use atlas_engine_utils_absolute_path::AbsolutePathBuf;
use atlas_engine_utils_path_uri::PathUri;
use std::collections::HashMap;

#[derive(Debug)]
pub(crate) struct ExecOptions {
    pub(crate) expiration: ExecExpiration,
    pub(crate) capture_policy: ExecCapturePolicy,
}

#[derive(Clone, Debug)]
pub(crate) struct ExecServerEnvConfig {
    pub(crate) policy: atlas_engine_exec_server::ExecEnvPolicy,
    pub(crate) local_policy_env: HashMap<String, String>,
}

#[derive(Debug)]
pub struct ExecRequest {
    pub command: Vec<String>,
    pub cwd: PathUri,
    pub env: HashMap<String, String>,
    pub(crate) exec_server_env_config: Option<ExecServerEnvConfig>,
    pub network: Option<NetworkProxy>,
    pub network_environment_id: Option<String>,
    pub expiration: ExecExpiration,
    pub capture_policy: ExecCapturePolicy,
    pub sandbox: SandboxType,
    pub windows_sandbox_policy_cwd: PathUri,
    pub windows_sandbox_workspace_roots: Vec<AbsolutePathBuf>,
    pub windows_sandbox_level: WindowsSandboxLevel,
    pub windows_sandbox_private_desktop: bool,
    pub permission_profile: PermissionProfile,
    pub(crate) windows_sandbox_filesystem_overrides: Option<WindowsSandboxFilesystemOverrides>,
    pub arg0: Option<String>,
    pub(crate) exec_server_sandbox: Option<FileSystemSandboxContext>,
    pub(crate) exec_server_enforce_managed_network: bool,
    pub(crate) exec_server_managed_network: Option<ManagedNetworkSandboxContext>,
    pub(crate) exec_server_network_proxy: Option<RemoteNetworkProxyLaunchConfig>,
}

impl ExecRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: Vec<String>,
        cwd: AbsolutePathBuf,
        env: HashMap<String, String>,
        network: Option<NetworkProxy>,
        network_environment_id: Option<String>,
        expiration: ExecExpiration,
        capture_policy: ExecCapturePolicy,
        sandbox: SandboxType,
        windows_sandbox_workspace_roots: Vec<AbsolutePathBuf>,
        windows_sandbox_level: WindowsSandboxLevel,
        windows_sandbox_private_desktop: bool,
        permission_profile: PermissionProfile,
        arg0: Option<String>,
    ) -> Self {
        let cwd = PathUri::from_abs_path(&cwd);
        let windows_sandbox_policy_cwd = cwd.clone();
        Self {
            command,
            cwd,
            env,
            exec_server_env_config: None,
            network,
            network_environment_id,
            expiration,
            capture_policy,
            sandbox,
            windows_sandbox_policy_cwd,
            windows_sandbox_workspace_roots,
            windows_sandbox_level,
            windows_sandbox_private_desktop,
            permission_profile,
            windows_sandbox_filesystem_overrides: None,
            arg0,
            exec_server_sandbox: None,
            exec_server_enforce_managed_network: false,
            exec_server_managed_network: None,
            exec_server_network_proxy: None,
        }
    }

    pub(crate) fn from_sandbox_exec_request(
        request: SandboxExecRequest,
        options: ExecOptions,
        windows_sandbox_workspace_roots: Vec<AbsolutePathBuf>,
    ) -> Self {
        let SandboxExecRequest {
            command,
            cwd,
            sandbox_policy_cwd: windows_sandbox_policy_cwd,
            mut env,
            network,
            network_environment_id,
            sandbox,
            windows_sandbox_level,
            windows_sandbox_private_desktop,
            permission_profile,
            arg0,
            ..
        } = request;
        let ExecOptions {
            expiration,
            capture_policy,
        } = options;
        let network_sandbox_policy = permission_profile.network_sandbox_policy();
        if !network_sandbox_policy.is_enabled() {
            env.insert(
                ATLAS_AGENT_SANDBOX_NETWORK_DISABLED_ENV_VAR.to_string(),
                "1".to_string(),
            );
        }
        #[cfg(target_os = "macos")]
        if sandbox == SandboxType::MacosSeatbelt {
            env.insert(ATLAS_AGENT_SANDBOX_ENV_VAR.to_string(), "seatbelt".to_string());
        }
        Self {
            command,
            cwd,
            env,
            exec_server_env_config: None,
            network,
            network_environment_id,
            expiration,
            capture_policy,
            sandbox,
            windows_sandbox_policy_cwd,
            windows_sandbox_workspace_roots,
            windows_sandbox_level,
            windows_sandbox_private_desktop,
            permission_profile,
            windows_sandbox_filesystem_overrides: None,
            arg0,
            exec_server_sandbox: None,
            exec_server_enforce_managed_network: false,
            exec_server_managed_network: None,
            exec_server_network_proxy: None,
        }
    }
}

pub async fn execute_env(
    exec_request: ExecRequest,
    stdout_stream: Option<StdoutStream>,
) -> atlas_engine_protocol::error::Result<ExecToolCallOutput> {
    execute_exec_request(exec_request, stdout_stream, /*after_spawn*/ None).await
}

pub async fn execute_exec_request_with_after_spawn(
    exec_request: ExecRequest,
    stdout_stream: Option<StdoutStream>,
    after_spawn: Option<Box<dyn FnOnce() + Send>>,
) -> atlas_engine_protocol::error::Result<ExecToolCallOutput> {
    execute_exec_request(exec_request, stdout_stream, after_spawn).await
}
