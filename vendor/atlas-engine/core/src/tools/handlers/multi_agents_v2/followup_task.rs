// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use super::message_tool::FollowupTaskArgs;
use super::message_tool::MessageDeliveryMode;
use super::message_tool::handle_message_string_tool;
use super::*;
use crate::tools::handlers::multi_agents_spec::create_followup_task_tool;
use atlas_engine_tools::ToolSpec;

pub(crate) struct Handler;

impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("followup_task")
    }

    fn spec(&self) -> ToolSpec {
        create_followup_task_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> atlas_engine_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl Handler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let arguments = function_arguments(invocation.payload.clone())?;
        let args: FollowupTaskArgs = parse_arguments(&arguments)?;
        handle_message_string_tool(
            invocation,
            MessageDeliveryMode::TriggerTurn,
            args.target,
            args.message,
        )
        .await
        .map(boxed_tool_output)
    }
}

impl CoreToolRuntime for Handler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}
