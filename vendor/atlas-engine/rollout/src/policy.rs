// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use crate::RolloutItem;
use crate::protocol::EventMsg;
use atlas_engine_extension_items::ExtensionItem;
use atlas_engine_protocol::items::TurnItem;
use atlas_engine_protocol::models::ResponseItem;
use atlas_engine_protocol::protocol::ThreadHistoryMode;

/// Whether a rollout `item` should be persisted in rollout files.
pub fn is_persisted_rollout_item(item: &RolloutItem, history_mode: ThreadHistoryMode) -> bool {
    match item {
        RolloutItem::ResponseItem(item) => should_persist_response_item(&item.item),
        RolloutItem::InterAgentCommunication(_)
        | RolloutItem::InterAgentCommunicationMetadata { .. } => true,
        RolloutItem::EventMsg(ev) => should_persist_event_msg(ev, history_mode),
        // Persist Atlas Agent executive markers so we can analyze flows (e.g., compaction, API turns).
        RolloutItem::Compacted(_)
        | RolloutItem::TurnContext(_)
        | RolloutItem::WorldState(_)
        | RolloutItem::SecurityRiskScore(_)
        | RolloutItem::SessionMeta(_) => true,
    }
}

/// Return the rollout items that should be persisted for a live append.
pub fn persisted_rollout_items(
    items: &[RolloutItem],
    history_mode: ThreadHistoryMode,
) -> Vec<RolloutItem> {
    let mut persisted = Vec::new();
    for item in items {
        if is_persisted_rollout_item(item, history_mode) {
            persisted.push(item.clone());
        }
    }
    persisted
}

/// Whether a `ResponseItem` should be persisted in rollout files.
#[inline]
pub fn should_persist_response_item(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::Message { .. }
        | ResponseItem::AgentMessage { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::LocalShellCall { .. }
        | ResponseItem::FunctionCall { .. }
        | ResponseItem::ToolSearchCall { .. }
        | ResponseItem::FunctionCallOutput { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::CustomToolCall { .. }
        | ResponseItem::CustomToolCallOutput { .. }
        | ResponseItem::WebSearchCall { .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::ContextCompaction { .. } => true,
        ResponseItem::AdditionalTools { .. }
        | ResponseItem::CompactionTrigger { .. }
        | ResponseItem::Other => false,
    }
}

/// Whether a `ResponseItem` should be persisted for the memories.
#[inline]
pub fn should_persist_response_item_for_memories(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::Message { role, .. } => role != "developer",
        ResponseItem::AgentMessage { .. }
        | ResponseItem::LocalShellCall { .. }
        | ResponseItem::FunctionCall { .. }
        | ResponseItem::ToolSearchCall { .. }
        | ResponseItem::FunctionCallOutput { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::CustomToolCall { .. }
        | ResponseItem::CustomToolCallOutput { .. }
        | ResponseItem::WebSearchCall { .. } => true,
        ResponseItem::AdditionalTools { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::CompactionTrigger { .. }
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::Other => false,
    }
}

/// Whether an `EventMsg` should be persisted in rollout files.
#[inline]
pub fn should_persist_event_msg(ev: &EventMsg, history_mode: ThreadHistoryMode) -> bool {
    match ev {
        EventMsg::ItemCompleted(event) => {
            // Paginated rollouts store TurnItems.
            // Legacy rollouts keep only items with no raw ResponseItem or legacy equivalent.
            matches!(history_mode, ThreadHistoryMode::Paginated)
                || matches!(
                    event.item,
                    TurnItem::Plan(_) | TurnItem::Extension(ExtensionItem::Sleep(_))
                )
        }
        EventMsg::TokenCount(_)
        | EventMsg::ThreadGoalUpdated(_)
        | EventMsg::ThreadRolledBack(_)
        | EventMsg::TurnAborted(_)
        | EventMsg::TurnStarted(_)
        | EventMsg::TurnComplete(_)
        | EventMsg::ThreadSettingsApplied(_) => true,

        // Only persist these legacy events when the thread's history mode is Legacy.
        // New, paginated rollouts persist ItemCompleted events with TurnItems.
        EventMsg::UserMessage(_)
        | EventMsg::AgentMessage(_)
        | EventMsg::AgentReasoning(_)
        | EventMsg::AgentReasoningRawContent(_)
        | EventMsg::EnteredReviewMode(_)
        | EventMsg::ExitedReviewMode(_)
        | EventMsg::PatchApplyEnd(_)
        // Atlas: shell commands are part of the record of a turn, and a
        // reopened thread that shows the file edits but not the commands
        // misrepresents what the agent did. The end event alone rebuilds a
        // complete item (`build_command_execution_end_item`) — command, cwd,
        // exit code and output — and the history builder already handles it,
        // so persisting it is all that was missing. Its BEGIN stays transient:
        // an unmatched begin would replay as a command still running.
        | EventMsg::ExecCommandEnd(_)
        | EventMsg::ContextCompacted(_)
        | EventMsg::McpToolCallEnd(_)
        | EventMsg::WebSearchEnd(_)
        | EventMsg::ImageGenerationEnd(_)
        | EventMsg::SubAgentActivity(_) => matches!(history_mode, ThreadHistoryMode::Legacy),

        // Transient, non-durable events.
        EventMsg::Error(_)
        | EventMsg::ThreadQueueChanged(_)
        | EventMsg::GuardianAssessment(_)
        | EventMsg::ViewImageToolCall(_)
        | EventMsg::CollabAgentSpawnEnd(_)
        | EventMsg::CollabAgentInteractionEnd(_)
        | EventMsg::CollabWaitingEnd(_)
        | EventMsg::CollabCloseEnd(_)
        | EventMsg::CollabResumeEnd(_)
        | EventMsg::DynamicToolCallRequest(_)
        | EventMsg::DynamicToolCallResponse(_)
        | EventMsg::Warning(_)
        | EventMsg::GuardianWarning(_)
        | EventMsg::RealtimeConversationStarted(_)
        | EventMsg::RealtimeConversationSdp(_)
        | EventMsg::RealtimeConversationRealtime(_)
        | EventMsg::RealtimeConversationClosed(_)
        | EventMsg::SafetyBuffering(_)
        | EventMsg::ModelReroute(_)
        | EventMsg::ModelVerification(_)
        | EventMsg::TurnModerationMetadata(_)
        | EventMsg::AgentReasoningSectionBreak(_)
        | EventMsg::RawResponseItem(_)
        | EventMsg::RawResponseCompleted(_)
        | EventMsg::SessionConfigured(_)
        | EventMsg::EnvironmentConnected(_)
        | EventMsg::EnvironmentDisconnected(_)
        | EventMsg::McpToolCallBegin(_)
        | EventMsg::ExecCommandBegin(_)
        | EventMsg::TerminalInteraction(_)
        | EventMsg::ExecCommandOutputDelta(_)
        | EventMsg::ExecApprovalRequest(_)
        | EventMsg::RequestPermissions(_)
        | EventMsg::RequestUserInput(_)
        | EventMsg::ElicitationRequest(_)
        | EventMsg::ApplyPatchApprovalRequest(_)
        | EventMsg::StreamError(_)
        | EventMsg::PatchApplyBegin(_)
        | EventMsg::PatchApplyUpdated(_)
        | EventMsg::TurnDiff(_)
        | EventMsg::RealtimeConversationListVoicesResponse(_)
        | EventMsg::McpStartupUpdate(_)
        | EventMsg::McpStartupComplete(_)
        | EventMsg::WebSearchBegin(_)
        | EventMsg::PlanUpdate(_)
        | EventMsg::ShutdownComplete
        | EventMsg::DeprecationNotice(_)
        | EventMsg::ItemStarted(_)
        | EventMsg::HookStarted(_)
        | EventMsg::HookCompleted(_)
        | EventMsg::AgentMessageContentDelta(_)
        | EventMsg::PlanDelta(_)
        | EventMsg::ReasoningContentDelta(_)
        | EventMsg::ReasoningRawContentDelta(_)
        | EventMsg::ImageGenerationBegin(_)
        | EventMsg::CollabAgentSpawnBegin(_)
        | EventMsg::CollabAgentInteractionBegin(_)
        | EventMsg::CollabWaitingBegin(_)
        | EventMsg::CollabCloseBegin(_)
        | EventMsg::CollabResumeBegin(_) => false,
    }
}

#[cfg(test)]
mod atlas_exec_persistence_tests {
    use super::*;

    fn exec_command_end() -> EventMsg {
        serde_json::from_value(serde_json::json!({
            "type": "exec_command_end",
            "call_id": "call-1",
            "turn_id": "turn-1",
            "command": ["ls", "-la"],
            "cwd": "file:///tmp/project",
            "parsed_cmd": [],
            "stdout": "Cargo.toml\n",
            "stderr": "",
            "aggregated_output": "Cargo.toml\n",
            "exit_code": 0,
            "duration": {"secs": 0, "nanos": 12_000_000},
            "formatted_output": "Cargo.toml\n",
            "status": "completed",
        }))
        .expect("a shell completion the protocol accepts")
    }

    /// A reopened thread that shows the file edits an agent made but none of
    /// the commands it ran misrepresents the turn. The completion carries
    /// everything the item needs, and the history builder already knows how to
    /// replay it — persisting it was the only missing piece.
    #[test]
    fn a_shell_completion_is_durable_on_a_legacy_thread() {
        assert!(should_persist_event_msg(
            &exec_command_end(),
            ThreadHistoryMode::Legacy
        ));
    }

    /// Paginated rollouts carry the same command as an `ItemCompleted`, so
    /// persisting the legacy event too would write it twice.
    #[test]
    fn a_paginated_thread_does_not_persist_it_twice() {
        assert!(!should_persist_event_msg(
            &exec_command_end(),
            ThreadHistoryMode::Paginated
        ));
    }

    /// The BEGIN stays transient on purpose: a begin with no matching end —
    /// which is exactly what a crash leaves behind — would replay as a command
    /// that is still running.
    #[test]
    fn the_start_of_a_command_is_still_not_persisted() {
        let begin: EventMsg = serde_json::from_value(serde_json::json!({
            "type": "exec_command_begin",
            "call_id": "call-1",
            "turn_id": "turn-1",
            "command": ["ls"],
            "cwd": "file:///tmp/project",
            "parsed_cmd": [],
        }))
        .expect("a shell start the protocol accepts");
        for mode in [ThreadHistoryMode::Legacy, ThreadHistoryMode::Paginated] {
            assert!(!should_persist_event_msg(&begin, mode));
        }
    }
}
