// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::sync::Arc;

use atlas_engine_extension_api::ExtensionMetrics;
use atlas_engine_otel::SessionTelemetry;

struct SessionTelemetryExtensionMetrics {
    session_telemetry: SessionTelemetry,
}

impl ExtensionMetrics for SessionTelemetryExtensionMetrics {
    fn histogram(&self, name: &str, value: i64, tags: &[(&str, &str)]) {
        self.session_telemetry.histogram(name, value, tags);
    }
}

pub(super) fn from_session_telemetry(
    session_telemetry: SessionTelemetry,
) -> Arc<dyn ExtensionMetrics> {
    Arc::new(SessionTelemetryExtensionMetrics { session_telemetry })
}
