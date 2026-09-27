// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use super::*;
use atlas_engine_protocol::approvals::NetworkPolicyAmendment;
use pretty_assertions::assert_eq;

#[test]
fn approval_resolution_rejects_denied_network_policy_amendment() {
    let resolution = ApprovalResolution {
        decision: ReviewDecision::NetworkPolicyAmendment {
            network_policy_amendment: NetworkPolicyAmendment {
                host: "denied.example.com".to_string(),
                action: NetworkPolicyRuleAction::Deny,
            },
        },
        source: ApprovalResolutionSource::User,
    };

    assert!(matches!(
        resolution.into_tool_result(),
        Err(ToolError::Rejected(rejection)) if rejection == "rejected by user"
    ));
}

#[test]
fn approval_resolution_rejects_mcp_policy_amendment() {
    let resolution = ApprovalResolution {
        decision: ReviewDecision::ApprovedMcpPolicyAmendment,
        source: ApprovalResolutionSource::User,
    };

    assert!(matches!(
        resolution.into_tool_result(),
        Err(ToolError::Rejected(rejection)) if rejection == "Error while requesting approval"
    ));
}

#[test]
fn approval_resolution_aborts_turn_when_approval_is_aborted() {
    let resolution = ApprovalResolution {
        decision: ReviewDecision::Abort,
        source: ApprovalResolutionSource::User,
    };

    assert!(matches!(
        resolution.into_tool_result(),
        Err(ToolError::AtlasEngine(error))
            if matches!(
                error.details(),
                atlas_engine_protocol::error::AtlasEngineErrorDetails::TurnAborted
            )
    ));
}

#[test]
fn guardian_cwd_preserves_drive_shaped_local_posix_path() {
    let native_cwd = AbsolutePathBuf::try_from(std::path::PathBuf::from("/C:/workspace"))
        .expect("drive-shaped POSIX path should be absolute");
    let cwd = PathUri::from_abs_path(&native_cwd);

    assert_eq!(
        guardian_cwd(atlas_engine_exec_server::LOCAL_ENVIRONMENT_ID, cwd)
            .expect("local cwd should retain the host path convention"),
        native_cwd
    );
}

#[test]
fn guardian_cwd_rejects_foreign_remote_path() {
    let cwd = PathUri::parse("file:///C:/workspace").expect("valid Windows path URI");

    assert!(guardian_cwd(atlas_engine_exec_server::REMOTE_ENVIRONMENT_ID, cwd).is_err());
}
