// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
pub(crate) const OTEL_TARGET_PREFIX: &str = "atlas_engine_otel";
pub(crate) const OTEL_LOG_ONLY_TARGET: &str = "atlas_engine_otel.log_only";
pub(crate) const OTEL_TRACE_SAFE_TARGET: &str = "atlas_engine_otel.trace_safe";

pub(crate) fn is_log_export_target(target: &str) -> bool {
    target.starts_with(OTEL_TARGET_PREFIX) && !is_trace_safe_target(target)
}

pub(crate) fn is_trace_safe_target(target: &str) -> bool {
    target.starts_with(OTEL_TRACE_SAFE_TARGET)
}
