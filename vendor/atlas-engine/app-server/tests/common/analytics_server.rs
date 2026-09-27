// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use anyhow::Result;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

pub async fn start_analytics_events_server() -> Result<MockServer> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/atlas-agent/analytics-events/events"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Ok(server)
}
