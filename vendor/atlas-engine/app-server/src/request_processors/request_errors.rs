// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use super::*;
use atlas_engine_protocol::error::AtlasEngineErrorDetails;

pub(super) fn environment_selection_error(err: AtlasEngineErr) -> JSONRPCErrorError {
    match err.details() {
        AtlasEngineErrorDetails::InvalidRequest(message) => invalid_request(message.clone()),
        _ => internal_error(format!("failed to validate environment selections: {err}")),
    }
}
