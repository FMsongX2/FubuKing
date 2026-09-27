// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use super::*;

use atlas_engine_connectors::ConnectorRuntimeTool;
use atlas_engine_connectors::connector_runtime_context_key;
use atlas_engine_connectors::connector_tool_is_synthetic;
use atlas_engine_connectors::installed_connector_runtime;
use atlas_engine_mcp::ATLAS_APPS_MCP_SERVER_NAME;
use atlas_engine_mcp::MCP_TOOL_ATLAS_APPS_META_KEY;
use atlas_engine_mcp::McpRuntime;
use atlas_engine_mcp::McpRuntimeInput;
use atlas_engine_mcp::McpStartupPolicy;
use atlas_engine_mcp::ToolInfo;
use atlas_engine_mcp::effective_mcp_servers;
use atlas_engine_mcp::host_owned_atlas_apps_enabled;
use atlas_engine_mcp::tool_is_model_visible;
use atlas_engine_protocol::mcp::ClientMcpExtensions;
use atlas_engine_protocol::models::PermissionProfile;

#[cfg(test)]
#[path = "installed_tests.rs"]
mod tests;

const CONNECTOR_RUNTIME_REFRESH_TIMEOUT: Duration = Duration::from_secs(30);
const APPS_INSTALLED_SUBMIT_ID: &str = "app-installed";
const APPS_INSTALLED_RESPONSE_BYTES_METRIC: &str = "atlas_agent.apps.installed.response_bytes";
const APPS_INSTALLED_CONNECTOR_COUNT_METRIC: &str = "atlas_agent.apps.installed.connector_count";
const APPS_INSTALLED_TOOL_COUNT_METRIC: &str = "atlas_agent.apps.installed.tool_count";
const APPS_SNAPSHOT_AGE_METRIC: &str = "atlas_agent.apps.snapshot.age_ms";

struct AppsInstalledSnapshotMetrics {
    age: Option<Duration>,
    tool_count: usize,
}

impl AppsRequestProcessor {
    pub(crate) async fn apps_installed(
        &self,
        params: AppsInstalledParams,
    ) -> Result<AppsInstalledResponse, JSONRPCErrorError> {
        let started_at = Instant::now();
        let force_refresh = params.force_refresh;
        let mut retained_previous_snapshot = false;
        let mut refresh_disposition = if force_refresh {
            "not_started"
        } else {
            "not_requested"
        };
        let mut snapshot_age = None;
        let mut snapshot_tool_count = 0;
        let result = async {
            let config = self
                .load_apps_config(params.thread_id.as_deref())
                .await?;
            let auth = self.auth_manager.auth().await;
            let apps_enabled = config
                .features
                .apps_enabled_for_auth(auth.as_ref().is_some_and(AtlasEngineAuth::uses_atlas_engine_backend));

            let workspace_enabled = apps_enabled
                && self
                    .workspace_atlas_engine_plugins_enabled(&config, auth.as_ref())
                    .await;
            let runtime_enabled = apps_enabled && workspace_enabled;

            let mcp_manager = self.thread_manager.mcp_manager();
            let mut mcp_config = mcp_manager.runtime_config(&config).await;
            // Installed-app discovery has no active turn or reviewer.
            mcp_config.permission_profile = PermissionProfile::default();
            let mcp_config = Arc::new(mcp_config);
            let mut mcp_servers = effective_mcp_servers(&mcp_config, auth.as_ref());
            mcp_servers.retain(|name, _| name == ATLAS_APPS_MCP_SERVER_NAME);
            let cache_key = connector_runtime_context_key(auth.as_ref());
            let previous_snapshot = mcp_manager
                .atlas_apps_tools_cache()
                .current_snapshot(config.atlas_agent_home.to_path_buf(), cache_key.clone());
            let snapshot = if force_refresh && runtime_enabled {
                let refresh_result = async {
                    anyhow::ensure!(
                        !mcp_servers.is_empty(),
                        "host-owned MCP server '{ATLAS_APPS_MCP_SERVER_NAME}' is not enabled"
                    );
                    let startup_timeout = mcp_servers
                        .get(ATLAS_APPS_MCP_SERVER_NAME)
                        .and_then(|server| server.config().startup_timeout_sec)
                        .unwrap_or(CONNECTOR_RUNTIME_REFRESH_TIMEOUT);
                    let runtime_context = McpRuntimeContext::new(
                        self.thread_manager.environment_manager(),
                        config.cwd.to_path_buf(),
                    );
                    let cancellation_token = CancellationToken::new();
                    let atlas_apps_auth_manager =
                        host_owned_atlas_apps_enabled(&mcp_config, auth.as_ref())
                            .then(|| Arc::clone(&self.auth_manager));
                    let runtime = McpRuntime::new(McpRuntimeInput {
                        startup_policy: McpStartupPolicy::Eager,
                        config: Arc::clone(&mcp_config),
                        plugins_available: false,
                        ready_selected_capability_roots: Vec::new(),
                        mcp_servers,
                        submit_id: APPS_INSTALLED_SUBMIT_ID.to_string(),
                        tx_event: None,
                        startup_cancellation_token: cancellation_token.clone(),
                        runtime_context,
                        atlas_apps_tools_cache: mcp_manager.atlas_apps_tools_cache(),
                        tool_catalog_cache: mcp_manager.tool_catalog_cache(),
                        atlas_apps_tools_cache_key: cache_key.clone(),
                        client_mcp_extensions: ClientMcpExtensions::default(),
                        auth: auth.clone(),
                        atlas_apps_auth_manager,
                        elicitation_reviewer: None,
                        elicitation_lifecycle: None,
                    })
                    .await;

                    let result = if runtime
                        .latest_wait_for_server_ready(
                            ATLAS_APPS_MCP_SERVER_NAME,
                            startup_timeout,
                        )
                        .await
                    {
                        mcp_manager
                            .atlas_apps_tools_cache()
                            .current_snapshot(config.atlas_agent_home.to_path_buf(), cache_key.clone())
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "hosted connector refresh completed without publishing a snapshot"
                                )
                            })
                    } else {
                        Err(anyhow::anyhow!(
                            "failed to refresh tools for MCP server '{ATLAS_APPS_MCP_SERVER_NAME}'"
                        ))
                    };
                    cancellation_token.cancel();
                    runtime.shutdown().await;
                    result
                }
                .await;

                match refresh_result {
                    Ok(snapshot) => {
                        refresh_disposition = "success";
                        Some(snapshot)
                    }
                    Err(err) => {
                        refresh_disposition = "error";
                        retained_previous_snapshot = previous_snapshot.is_some();
                        return Err(internal_error(format!(
                            "failed to refresh installed connector runtime state: {err:#}"
                        )));
                    }
                }
            } else {
                if force_refresh {
                    refresh_disposition = if !apps_enabled {
                        "skipped_apps_disabled"
                    } else {
                        "skipped_workspace_disabled"
                    };
                    retained_previous_snapshot = previous_snapshot.is_some();
                }
                previous_snapshot
            };
            let Some(snapshot) = snapshot else {
                return Ok(AppsInstalledResponse { apps: Vec::new() });
            };

            snapshot_age = Some(snapshot.age());
            snapshot_tool_count = snapshot.tools().len();
            let apps = installed_connector_runtime(
                &config.config_layer_stack,
                snapshot.tools().iter().map(connector_runtime_tool),
            )
            .into_iter()
            .map(|app| InstalledApp {
                id: app.id,
                runtime_name: app.runtime_name,
                enabled: runtime_enabled && app.enabled,
                callable: runtime_enabled && app.callable,
            })
            .collect();
            Ok(AppsInstalledResponse { apps })
        }
        .await;

        if let Some(metrics) = atlas_engine_otel::global() {
            record_apps_installed_metrics(
                &metrics,
                started_at,
                force_refresh,
                retained_previous_snapshot,
                refresh_disposition,
                AppsInstalledSnapshotMetrics {
                    age: snapshot_age,
                    tool_count: snapshot_tool_count,
                },
                result.as_ref().ok(),
            );
        }
        result
    }
}

fn connector_runtime_tool(tool: &ToolInfo) -> ConnectorRuntimeTool<'_> {
    let annotations = tool.tool.annotations.as_ref();
    ConnectorRuntimeTool {
        connector_id: tool.connector_id.as_deref(),
        connector_name: tool.connector_name.as_deref(),
        tool_name: &tool.tool.name,
        tool_title: tool.tool.title.as_deref(),
        destructive_hint: annotations.and_then(|annotations| annotations.destructive_hint),
        open_world_hint: annotations.and_then(|annotations| annotations.open_world_hint),
        synthetic: connector_tool_is_synthetic(
            tool.tool
                .meta
                .as_deref()
                .and_then(|meta| meta.get(MCP_TOOL_ATLAS_APPS_META_KEY)),
        ),
        model_visible: tool_is_model_visible(tool),
    }
}

fn record_apps_installed_metrics(
    metrics: &atlas_engine_otel::MetricsClient,
    started_at: Instant,
    force_refresh: bool,
    retained_previous_snapshot: bool,
    refresh_disposition: &'static str,
    snapshot_metrics: AppsInstalledSnapshotMetrics,
    response: Option<&AppsInstalledResponse>,
) {
    let Some(response) = response else {
        return;
    };
    let force_refresh = if force_refresh { "true" } else { "false" };
    let retained_previous_snapshot = if retained_previous_snapshot {
        "true"
    } else {
        "false"
    };
    let _ = metrics.record_duration(
        APPS_INSTALLED_DURATION_METRIC,
        started_at.elapsed(),
        &[
            ("path", "installed"),
            ("reload", force_refresh),
            ("force_refresh", force_refresh),
            ("refresh", refresh_disposition),
            ("outcome", "success"),
            ("retained_previous_snapshot", retained_previous_snapshot),
        ],
    );
    if let Ok(bytes) = serde_json::to_vec(response) {
        let _ = metrics.histogram(
            APPS_INSTALLED_RESPONSE_BYTES_METRIC,
            i64::try_from(bytes.len()).unwrap_or(i64::MAX),
            &[("path", "new")],
        );
    }
    let _ = metrics.histogram(
        APPS_INSTALLED_CONNECTOR_COUNT_METRIC,
        i64::try_from(response.apps.len()).unwrap_or(i64::MAX),
        &[("path", "new")],
    );
    let _ = metrics.histogram(
        APPS_INSTALLED_TOOL_COUNT_METRIC,
        i64::try_from(snapshot_metrics.tool_count).unwrap_or(i64::MAX),
        &[("path", "new")],
    );
    if let Some(snapshot_age) = snapshot_metrics.age {
        let _ = metrics.record_duration(
            APPS_SNAPSHOT_AGE_METRIC,
            snapshot_age,
            &[("path", "new"), ("observation", "installed")],
        );
    }
}
