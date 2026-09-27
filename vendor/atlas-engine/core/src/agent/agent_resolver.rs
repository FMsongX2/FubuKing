// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use atlas_engine_protocol::ThreadId;
use atlas_engine_protocol::error::AtlasEngineErrorDetails;
use std::sync::Arc;

/// Resolves a single tool-facing agent target to a thread id.
pub(crate) async fn resolve_agent_target(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    target: &str,
) -> Result<ThreadId, FunctionCallError> {
    register_session_root(session, turn);
    if let Ok(thread_id) = ThreadId::from_string(target) {
        return Ok(thread_id);
    }

    session
        .services
        .agent_control
        .resolve_agent_reference(session.thread_id, &turn.session_source, target)
        .await
        .map_err(|err| match err.details() {
            AtlasEngineErrorDetails::UnsupportedOperation(message) => {
                FunctionCallError::RespondToModel(message.clone())
            }
            _ => FunctionCallError::RespondToModel(err.to_string()),
        })
}

fn register_session_root(session: &Arc<Session>, turn: &Arc<TurnContext>) {
    session
        .services
        .agent_control
        .register_session_root(session.thread_id, turn.parent_thread_id);
}
