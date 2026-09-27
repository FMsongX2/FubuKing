// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::collections::HashSet;

use atlas_engine_exec_server::MAX_SELECTED_CAPABILITY_ROOTS;
use atlas_engine_exec_server::SelectedCapabilityRootsStatus;
use atlas_engine_protocol::capabilities::CapabilityRootLocation;
use atlas_engine_protocol::error::AtlasEngineErr;
use atlas_engine_protocol::error::Result as AtlasEngineResult;
use atlas_engine_protocol::protocol::EnvironmentConfig;
use atlas_engine_protocol::protocol::EnvironmentConfigState;
use atlas_engine_protocol::protocol::TurnEnvironmentSelection;

use crate::config::ConstraintResult;
use crate::session::session::Session;
use crate::session::session::SessionConfiguration;
use crate::session::session::SessionSettingsUpdate;

pub(super) fn validate_environment_selections(
    selections: &[TurnEnvironmentSelection],
) -> AtlasEngineResult<()> {
    for selection in selections {
        match &selection.config {
            EnvironmentConfigState::FromThread => {}
            EnvironmentConfigState::Pending => {
                return Err(AtlasEngineErr::InvalidRequest(
                    "pending environment configuration is not supported yet".to_string(),
                ));
            }
            EnvironmentConfigState::Ready(config) => {
                validate_environment_config(selection, config)?;
            }
        }
    }
    Ok(())
}

fn validate_environment_config(
    selection: &TurnEnvironmentSelection,
    config: &EnvironmentConfig,
) -> AtlasEngineResult<()> {
    if config.selected_capability_roots.len() > MAX_SELECTED_CAPABILITY_ROOTS {
        return Err(AtlasEngineErr::InvalidRequest(format!(
            "environment readiness contains more than {MAX_SELECTED_CAPABILITY_ROOTS} selected capability roots"
        )));
    }

    let mut root_ids = HashSet::with_capacity(config.selected_capability_roots.len());
    for root in &config.selected_capability_roots {
        let CapabilityRootLocation::Environment { environment_id, .. } = &root.location;
        if root.id.trim().is_empty()
            || environment_id != &selection.environment_id
            || !root_ids.insert(root.id.as_str())
        {
            return Err(AtlasEngineErr::InvalidRequest(format!(
                "selected capability roots must have unique non-empty IDs and belong to environment `{}`",
                selection.environment_id
            )));
        }
    }
    Ok(())
}

impl Session {
    pub(super) fn apply_session_settings(
        &self,
        current: &SessionConfiguration,
        updates: &SessionSettingsUpdate,
    ) -> ConstraintResult<SessionConfiguration> {
        current.apply(updates, &self.services.turn_environments.selections())
    }

    pub(crate) async fn environment_ready(
        &self,
        selection: &TurnEnvironmentSelection,
        config: EnvironmentConfig,
    ) -> AtlasEngineResult<()> {
        validate_environment_config(selection, &config)?;

        // grab session lock so installation can't race w/ thread settings updates
        let _state = self.state.lock().await;
        self.services
            .turn_environments
            .environment_ready(selection, config)?;
        // mark mcp runtime for refresh because available capabilities could've changed
        self.mark_mcp_runtime_dirty();
        Ok(())
    }

    /// Combines this session's persisted roots with ready environment attachments.
    pub(crate) fn inspect_selected_capability_roots(&self) -> SelectedCapabilityRootsStatus {
        self.services
            .turn_environments
            .inspect_selected_capability_roots(&self.services.selected_capability_roots)
    }
}
