// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_exec_server::EnvironmentManager;
use atlas_engine_http_client::HttpClientFactory;
use atlas_engine_http_client::OutboundProxyPolicy;

/// Builds a manager without environments using the legacy outbound HTTP policy.
pub fn environment_manager_without_environments() -> EnvironmentManager {
    EnvironmentManager::without_environments(HttpClientFactory::new(
        OutboundProxyPolicy::ReqwestDefault,
    ))
}
