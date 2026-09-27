// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_protocol::user_input::UserInput;
use atlas_engine_utils_path_uri::PathUri;

/// Host-owned turn environment summary visible to turn-input contributors.
#[derive(Debug, Clone)]
pub struct TurnInputEnvironment {
    /// Stable host environment id used to route executor-scoped capabilities.
    pub environment_id: String,
    /// Effective working directory for this turn in the environment.
    pub cwd: PathUri,
    /// Whether this is the primary environment for the turn.
    pub is_primary: bool,
}

/// Turn facts supplied before the host records turn-local model input items.
#[derive(Debug, Clone)]
pub struct TurnInputContext {
    /// Stable host-owned turn identifier.
    pub turn_id: String,
    /// User input submitted for this turn.
    pub user_input: Vec<UserInput>,
    /// Resolved turn environments, in host priority order.
    pub environments: Vec<TurnInputEnvironment>,
}
