// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use atlas_engine_app_server_protocol::ClientRequest;
use atlas_engine_app_server_protocol::MarketplaceRemoveParams;
use atlas_engine_app_server_protocol::MarketplaceRemoveResponse;
use atlas_engine_app_server_protocol::RequestId;
use atlas_engine_config::MarketplaceConfigUpdate;
use atlas_engine_config::record_user_marketplace;
use atlas_engine_core_plugins::installed_marketplaces::marketplace_install_root;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

fn configured_marketplace_update() -> MarketplaceConfigUpdate<'static> {
    MarketplaceConfigUpdate {
        last_updated: "2026-04-13T00:00:00Z",
        last_revision: None,
        source_type: "git",
        source: "https://github.com/owner/repo.git",
        ref_name: Some("main"),
        sparse_paths: &[],
    }
}

fn write_installed_marketplace(atlas_agent_home: &std::path::Path, marketplace_name: &str) -> Result<()> {
    let root = marketplace_install_root(atlas_agent_home).join(marketplace_name);
    std::fs::create_dir_all(root.join(".agents/plugins"))?;
    std::fs::write(root.join(".agents/plugins/marketplace.json"), "{}")?;
    Ok(())
}

fn canonicalize_path_with_existing_parent(path: &std::path::Path) -> Result<std::path::PathBuf> {
    let parent = path
        .parent()
        .with_context(|| format!("path {} should have a parent", path.display()))?;
    let file_name = path
        .file_name()
        .with_context(|| format!("path {} should have a file name", path.display()))?;

    Ok(parent.canonicalize()?.join(file_name))
}

#[tokio::test]
async fn marketplace_remove_deletes_config_and_installed_root() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    record_user_marketplace(atlas_agent_home.path(), "debug", &configured_marketplace_update())?;
    write_installed_marketplace(atlas_agent_home.path(), "debug")?;
    let installed_root = marketplace_install_root(atlas_agent_home.path()).join("debug");

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;
    let response: MarketplaceRemoveResponse = mcp
        .request(|request_id| ClientRequest::MarketplaceRemove {
            request_id,
            params: MarketplaceRemoveParams {
                marketplace_name: "debug".to_string(),
            },
        })
        .await?;
    assert_eq!(response.marketplace_name, "debug");
    let removed_installed_root = response
        .installed_root
        .context("marketplace/remove should return removed installed root")?;
    assert_eq!(
        canonicalize_path_with_existing_parent(removed_installed_root.as_path())?,
        canonicalize_path_with_existing_parent(&installed_root)?,
    );

    let config = std::fs::read_to_string(atlas_agent_home.path().join("config.toml"))?;
    assert!(!config.contains("[marketplaces.debug]"));
    assert!(
        !marketplace_install_root(atlas_agent_home.path())
            .join("debug")
            .exists()
    );
    Ok(())
}

#[tokio::test]
async fn marketplace_remove_rejects_unknown_marketplace() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;

    let request_id = mcp
        .send_marketplace_remove_request(MarketplaceRemoveParams {
            marketplace_name: "debug".to_string(),
        })
        .await?;

    let err = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(err.error.code, -32600);
    assert_eq!(
        err.error.message,
        "marketplace `debug` is not configured or installed",
    );
    Ok(())
}
