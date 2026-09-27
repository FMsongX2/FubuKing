// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::time::Duration;

use anyhow::Result;
use app_test_support::TestAppServer;
use atlas_engine_app_server_protocol::PermissionProfileListParams;
use atlas_engine_app_server_protocol::PermissionProfileListResponse;
use atlas_engine_app_server_protocol::PermissionProfileSummary;
use atlas_engine_core::config::set_project_trust_level;
use atlas_engine_protocol::config_types::TrustLevel;
use atlas_engine_protocol::models::BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS;
use atlas_engine_protocol::models::BUILT_IN_PERMISSION_PROFILE_READ_ONLY;
use atlas_engine_protocol::models::BUILT_IN_PERMISSION_PROFILE_WORKSPACE;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test]
async fn permission_profile_list_returns_builtin_and_configured_profiles() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    std::fs::write(
        atlas_agent_home.path().join("config.toml"),
        r#"
default_permissions = "dev"

[permissions.dev]
description = "Day-to-day coding work."

[permissions.dev.filesystem]
":workspace_roots" = "write"

[permissions.audit]
description = "Inspect without writes."

[permissions.audit.filesystem]
":workspace_roots" = "read"
"#,
    )?;

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build_initialized_with_timeout(DEFAULT_TIMEOUT)
        .await?;

    let request_id = mcp
        .send_permission_profile_list_request(PermissionProfileListParams {
            cursor: None,
            limit: None,
            cwd: None,
        })
        .await?;
    let actual = read_response::<PermissionProfileListResponse>(&mut mcp, request_id).await?;

    assert_eq!(
        actual,
        PermissionProfileListResponse {
            data: vec![
                PermissionProfileSummary {
                    id: BUILT_IN_PERMISSION_PROFILE_READ_ONLY.to_string(),
                    description: None,
                    allowed: true,
                },
                PermissionProfileSummary {
                    id: BUILT_IN_PERMISSION_PROFILE_WORKSPACE.to_string(),
                    description: None,
                    allowed: true,
                },
                PermissionProfileSummary {
                    id: BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS.to_string(),
                    description: None,
                    allowed: true,
                },
                PermissionProfileSummary {
                    id: "audit".to_string(),
                    description: Some("Inspect without writes.".to_string()),
                    allowed: true,
                },
                PermissionProfileSummary {
                    id: "dev".to_string(),
                    description: Some("Day-to-day coding work.".to_string()),
                    allowed: true,
                },
            ],
            next_cursor: None,
        }
    );
    Ok(())
}

#[tokio::test]
async fn permission_profile_list_resolves_project_profiles_and_paginates() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    let workspace = TempDir::new()?;
    let project_config_dir = workspace.path().join(".atlas-agent");
    std::fs::create_dir_all(&project_config_dir)?;
    std::fs::write(
        atlas_agent_home.path().join("config.toml"),
        r#"
default_permissions = ":workspace"
"#,
    )?;
    std::fs::write(
        project_config_dir.join("config.toml"),
        r#"
[permissions.project]
description = "Project-scoped profile."

[permissions.project.filesystem]
":workspace_roots" = "write"
"#,
    )?;
    set_project_trust_level(atlas_agent_home.path(), workspace.path(), TrustLevel::Trusted)?;

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build_initialized_with_timeout(DEFAULT_TIMEOUT)
        .await?;

    let first_request_id = mcp
        .send_permission_profile_list_request(PermissionProfileListParams {
            cursor: None,
            limit: Some(3),
            cwd: Some(workspace.path().to_string_lossy().into_owned()),
        })
        .await?;
    let first = read_response::<PermissionProfileListResponse>(&mut mcp, first_request_id).await?;
    assert_eq!(
        first,
        PermissionProfileListResponse {
            data: vec![
                PermissionProfileSummary {
                    id: BUILT_IN_PERMISSION_PROFILE_READ_ONLY.to_string(),
                    description: None,
                    allowed: true,
                },
                PermissionProfileSummary {
                    id: BUILT_IN_PERMISSION_PROFILE_WORKSPACE.to_string(),
                    description: None,
                    allowed: true,
                },
                PermissionProfileSummary {
                    id: BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS.to_string(),
                    description: None,
                    allowed: true,
                },
            ],
            next_cursor: Some("3".to_string()),
        }
    );

    let second_request_id = mcp
        .send_permission_profile_list_request(PermissionProfileListParams {
            cursor: first.next_cursor,
            limit: Some(3),
            cwd: Some(workspace.path().to_string_lossy().into_owned()),
        })
        .await?;
    let second =
        read_response::<PermissionProfileListResponse>(&mut mcp, second_request_id).await?;
    assert_eq!(
        second,
        PermissionProfileListResponse {
            data: vec![PermissionProfileSummary {
                id: "project".to_string(),
                description: Some("Project-scoped profile.".to_string()),
                allowed: true,
            }],
            next_cursor: None,
        }
    );
    Ok(())
}

#[tokio::test]
async fn permission_profile_list_discovers_project_profiles_without_default_selection() -> Result<()>
{
    let atlas_agent_home = TempDir::new()?;
    let workspace = TempDir::new()?;
    let project_config_dir = workspace.path().join(".atlas-agent");
    std::fs::create_dir_all(&project_config_dir)?;
    std::fs::write(
        project_config_dir.join("config.toml"),
        r#"
[permissions.project]
description = "Project-scoped profile."

[permissions.project.filesystem]
":workspace_roots" = "write"
"#,
    )?;
    set_project_trust_level(atlas_agent_home.path(), workspace.path(), TrustLevel::Trusted)?;

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build_initialized_with_timeout(DEFAULT_TIMEOUT)
        .await?;

    let request_id = mcp
        .send_permission_profile_list_request(PermissionProfileListParams {
            cursor: None,
            limit: None,
            cwd: Some(workspace.path().to_string_lossy().into_owned()),
        })
        .await?;
    let actual = read_response::<PermissionProfileListResponse>(&mut mcp, request_id).await?;

    assert_eq!(
        actual,
        PermissionProfileListResponse {
            data: vec![
                PermissionProfileSummary {
                    id: BUILT_IN_PERMISSION_PROFILE_READ_ONLY.to_string(),
                    description: None,
                    allowed: true,
                },
                PermissionProfileSummary {
                    id: BUILT_IN_PERMISSION_PROFILE_WORKSPACE.to_string(),
                    description: None,
                    allowed: true,
                },
                PermissionProfileSummary {
                    id: BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS.to_string(),
                    description: None,
                    allowed: true,
                },
                PermissionProfileSummary {
                    id: "project".to_string(),
                    description: Some("Project-scoped profile.".to_string()),
                    allowed: true,
                },
            ],
            next_cursor: None,
        }
    );
    Ok(())
}

async fn read_response<T: serde::de::DeserializeOwned>(
    mcp: &mut TestAppServer,
    request_id: i64,
) -> Result<T> {
    timeout(DEFAULT_TIMEOUT, mcp.read_response(request_id)).await?
}
