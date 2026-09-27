// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_tools::JsonSchema;
use atlas_engine_tools::ResponsesApiTool;
use atlas_engine_tools::ToolSpec;
use std::collections::BTreeMap;

pub(crate) const NEW_CONTEXT_WINDOW_TOOL_NAME: &str = "new_context";

pub fn create_new_context_window_tool() -> ToolSpec {
    ToolSpec::Function(ResponsesApiTool {
        name: NEW_CONTEXT_WINDOW_TOOL_NAME.to_string(),
        description: "Start a new context window. Does not clear, reset, or otherwise affect environment state.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(BTreeMap::new(), /*required*/ None, Some(false.into())),
        output_schema: None,
    })
}
