// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use uuid::Uuid;

use super::*;

#[tokio::test]
async fn sqlite_sink_filters_noisy_targets_without_dropping_useful_diagnostics() {
    let atlas_agent_home =
        std::env::temp_dir().join(format!("atlas-engine-state-log-db-filter-{}", Uuid::new_v4()));
    let _cleanup = scopeguard::guard(atlas_agent_home.clone(), |atlas_agent_home| {
        let _ = std::fs::remove_dir_all(atlas_agent_home);
    });
    let runtime = StateRuntime::init(
        crate::SqliteConfig::new_for_testing(atlas_agent_home.as_path().abs()),
        "test-provider".to_string(),
    )
    .await
    .expect("initialize runtime");
    let layer = start(runtime.clone());

    let guard = tracing_subscriber::registry()
        .with(layer.clone().with_filter(default_filter()))
        .set_default();

    tracing::trace!(target: "opentelemetry_sdk", "dropped-trace");
    tracing::debug!(target: "opentelemetry_sdk", "dropped-debug");
    tracing::info!(target: "opentelemetry_sdk", "retained-info");
    tracing::debug!(target: "rmcp::transport", "dropped-rmcp-debug");
    tracing::info!(target: "rmcp::transport", "retained-rmcp-info");
    tracing::debug!(
        target: "atlas_engine_rmcp_client::oauth",
        "dropped-atlas-engine-rmcp-client-debug"
    );
    tracing::info!(
        target: "atlas_engine_rmcp_client::oauth",
        "retained-atlas-engine-rmcp-client-info"
    );
    tracing::trace!(target: "atlas_engine_http_client::transport", "dropped-request-body");
    tracing::debug!(target: "atlas_engine_http_client::transport", "retained-request-diagnostic");
    tracing::trace!(target: "atlas_engine_api::sse", "dropped-sse-parent");
    tracing::trace!(target: "atlas_engine_api::sse::responses", "dropped-sse-payload");
    tracing::debug!(target: "atlas_engine_api::sse::responses", "retained-sse-diagnostic");
    tracing::trace!(target: "atlas_engine_state", "retained-trace");
    tracing::trace!(
        target: "atlas_engine_tui::streaming::controller",
        "dropped-controller-trace"
    );
    tracing::debug!(
        target: "atlas_engine_tui::streaming::controller",
        "retained-controller-debug"
    );
    tracing::trace!(
        target: "atlas_engine_tui::streaming::table_holdback",
        "dropped-table-holdback-trace"
    );
    tracing::debug!(
        target: "atlas_engine_tui::streaming::table_holdback",
        "retained-table-holdback-debug"
    );
    tracing::trace!(
        target: "atlas_engine_tui::streaming::commit_tick",
        "retained-commit-tick-trace"
    );
    tracing::trace!(
        target: "atlas_engine_api::responses_websocket_timing",
        payload = "complete timing payload",
        "dropped-websocket-timing"
    );

    layer.flush().await;
    drop(guard);

    let logs = runtime
        .query_logs(&crate::LogQuery::default())
        .await
        .expect("query logs after flush");
    assert_eq!(
        logs.iter()
            .map(|row| (
                row.level.as_str(),
                row.target.as_str(),
                row.message.as_deref()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("INFO", "opentelemetry_sdk", Some("retained-info")),
            ("INFO", "rmcp::transport", Some("retained-rmcp-info")),
            (
                "INFO",
                "atlas_engine_rmcp_client::oauth",
                Some("retained-atlas-engine-rmcp-client-info")
            ),
            (
                "DEBUG",
                "atlas_engine_http_client::transport",
                Some("retained-request-diagnostic")
            ),
            (
                "DEBUG",
                "atlas_engine_api::sse::responses",
                Some("retained-sse-diagnostic")
            ),
            ("TRACE", "atlas_engine_state", Some("retained-trace")),
            (
                "DEBUG",
                "atlas_engine_tui::streaming::controller",
                Some("retained-controller-debug"),
            ),
            (
                "DEBUG",
                "atlas_engine_tui::streaming::table_holdback",
                Some("retained-table-holdback-debug"),
            ),
            (
                "TRACE",
                "atlas_engine_tui::streaming::commit_tick",
                Some("retained-commit-tick-trace"),
            ),
        ]
    );
}
