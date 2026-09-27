// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use super::session::Session;
use super::turn_context::TurnContext;
use crate::context::ContextualUserFragment;
use atlas_engine_protocol::error::AtlasEngineErr;
use atlas_engine_protocol::error::Result as AtlasEngineResult;
use atlas_engine_protocol::protocol::TokenUsage;

pub(super) async fn maybe_record_reminder(
    sess: &Session,
    turn_context: &TurnContext,
    window_id: &str,
) {
    let budget = sess.services.agent_control.rollout_budget();
    let Some(reminder) = budget.pending_reminder(sess.thread_id(), window_id) else {
        return;
    };
    let response_item = ContextualUserFragment::into(crate::context::RolloutBudgetContext {
        remaining_tokens: reminder.remaining_tokens,
    });
    sess.record_conversation_items(turn_context, std::slice::from_ref(&response_item))
        .await;
    budget.mark_reminder_delivered(sess.thread_id(), window_id, reminder);
}

impl Session {
    pub(crate) fn record_rollout_budget_usage(&self, usage: &TokenUsage) -> AtlasEngineResult<()> {
        if self
            .services
            .agent_control
            .rollout_budget()
            .record_usage(usage)?
        {
            return Err(AtlasEngineErr::SessionBudgetExceeded);
        }
        Ok(())
    }
}
