// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::sync::Arc;

use atlas_engine_analytics::AnalyticsEventsClient;
use atlas_engine_core::config::Config;
use atlas_engine_login::AuthManager;

pub(crate) fn analytics_events_client_from_config(
    auth_manager: Arc<AuthManager>,
    config: &Config,
) -> AnalyticsEventsClient {
    AnalyticsEventsClient::new(
        auth_manager,
        config.chatgpt_base_url.trim_end_matches('/').to_string(),
        config.analytics_enabled,
    )
}
