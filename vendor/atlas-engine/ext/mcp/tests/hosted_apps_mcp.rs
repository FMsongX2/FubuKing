// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::sync::Arc;

use atlas_engine_config::McpServerTransportConfig;
use atlas_engine_core::McpManager;
use atlas_engine_core::config::Config;
use atlas_engine_core::config::ConfigBuilder;
use atlas_engine_core::plugins_manager_for_config;
use atlas_engine_extension_api::ExtensionRegistryBuilder;
use atlas_engine_extension_api::McpServerContribution;
use atlas_engine_extension_api::McpServerContributionContext;
use atlas_engine_extension_api::McpServerContributor;
use atlas_engine_login::AtlasEngineAuth;
use atlas_engine_mcp::ATLAS_APPS_MCP_SERVER_NAME;
use pretty_assertions::assert_eq;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn contributes_hosted_plugin_runtime_without_an_executor() -> TestResult {
    let atlas_agent_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .atlas_agent_home(atlas_agent_home.path().to_path_buf())
        .fallback_cwd(Some(atlas_agent_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            ("chatgpt_base_url".to_string(), "https://chatgpt.com".into()),
        ])
        .build()
        .await?;
    let auth = AtlasEngineAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = installed_manager(&config, Some(auth.api_auth_mode()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(ATLAS_APPS_MCP_SERVER_NAME)
        .ok_or("hosted plugin runtime should be contributed as a configured server")?
        .config();
    let McpServerTransportConfig::StreamableHttp { url, .. } = &server.transport else {
        panic!("hosted plugin runtime should use streamable HTTP");
    };
    assert_eq!(url, "https://chatgpt.com/backend-api/ps/mcp");

    Ok(())
}

#[tokio::test]
async fn runtime_overlay_preserves_disabled_server() -> TestResult {
    let atlas_agent_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .atlas_agent_home(atlas_agent_home.path().to_path_buf())
        .fallback_cwd(Some(atlas_agent_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            (
                "mcp_servers.atlas_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
            ("mcp_servers.atlas_apps.enabled".to_string(), false.into()),
        ])
        .build()
        .await?;
    let auth = AtlasEngineAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = installed_manager(&config, Some(auth.api_auth_mode()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(ATLAS_APPS_MCP_SERVER_NAME)
        .ok_or("hosted plugin runtime should remain configured")?;

    assert!(!server.enabled());
    Ok(())
}

#[tokio::test]
async fn default_fallback_overwrites_reserved_config_without_an_extension() -> TestResult {
    let atlas_agent_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .atlas_agent_home(atlas_agent_home.path().to_path_buf())
        .fallback_cwd(Some(atlas_agent_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            (
                "mcp_servers.atlas_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
        ])
        .build()
        .await?;
    let auth = AtlasEngineAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = McpManager::new(Arc::new(plugins_manager_for_config(
        &config,
        Some(auth.api_auth_mode()),
    )));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(ATLAS_APPS_MCP_SERVER_NAME)
        .ok_or("default Apps MCP should be present")?
        .config();
    let McpServerTransportConfig::StreamableHttp { url, .. } = &server.transport else {
        panic!("default Apps MCP should use streamable HTTP");
    };
    assert_eq!(url, "https://chatgpt.com/backend-api/ps/mcp");

    Ok(())
}

#[tokio::test]
async fn later_extension_can_remove_same_name_registration() -> TestResult {
    let atlas_agent_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .atlas_agent_home(atlas_agent_home.path().to_path_buf())
        .fallback_cwd(Some(atlas_agent_home.path().to_path_buf()))
        .cli_overrides(vec![("features.apps".to_string(), true.into())])
        .build()
        .await?;
    let auth = AtlasEngineAuth::create_dummy_chatgpt_auth_for_testing();
    let mut builder = ExtensionRegistryBuilder::new();
    atlas_engine_mcp_extension::install(&mut builder);
    builder.mcp_server_contributor(Arc::new(RemoveAtlasApps));
    let manager = McpManager::new_with_extensions(
        Arc::new(plugins_manager_for_config(
            &config,
            Some(auth.api_auth_mode()),
        )),
        Arc::new(builder.build()),
        atlas_engine_core::AtlasAppsToolsCache::default(),
    );

    let servers = manager.effective_servers(&config, Some(&auth)).await;

    assert!(!servers.contains_key(ATLAS_APPS_MCP_SERVER_NAME));
    Ok(())
}

#[tokio::test]
async fn hosted_apps_mcp_requires_chatgpt_auth() -> TestResult {
    let atlas_agent_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .atlas_agent_home(atlas_agent_home.path().to_path_buf())
        .fallback_cwd(Some(atlas_agent_home.path().to_path_buf()))
        .cli_overrides(vec![("features.apps".to_string(), true.into())])
        .build()
        .await?;
    let auth = AtlasEngineAuth::from_api_key("test");
    let manager = installed_manager(&config, Some(auth.api_auth_mode()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    assert!(!servers.contains_key(ATLAS_APPS_MCP_SERVER_NAME));

    Ok(())
}

#[tokio::test]
async fn disabled_apps_remove_reserved_server_config_for_all_hosts() -> TestResult {
    let atlas_agent_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .atlas_agent_home(atlas_agent_home.path().to_path_buf())
        .fallback_cwd(Some(atlas_agent_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), false.into()),
            (
                "mcp_servers.atlas_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
        ])
        .build()
        .await?;
    let managers = [
        installed_manager(&config, /*auth_mode*/ None),
        McpManager::new(Arc::new(plugins_manager_for_config(
            &config, /*auth_mode*/ None,
        ))),
    ];
    for manager in managers {
        let servers = manager.runtime_servers(&config).await;
        assert!(!servers.contains_key(ATLAS_APPS_MCP_SERVER_NAME));
    }
    Ok(())
}

fn installed_manager(
    config: &Config,
    auth_mode: Option<atlas_engine_protocol::auth::AuthMode>,
) -> McpManager {
    let mut builder = ExtensionRegistryBuilder::new();
    atlas_engine_mcp_extension::install(&mut builder);
    McpManager::new_with_extensions(
        Arc::new(plugins_manager_for_config(config, auth_mode)),
        Arc::new(builder.build()),
        atlas_engine_core::AtlasAppsToolsCache::default(),
    )
}

struct RemoveAtlasApps;

impl McpServerContributor<Config> for RemoveAtlasApps {
    fn id(&self) -> &'static str {
        "remove_atlas_apps"
    }

    fn contribute<'a>(
        &'a self,
        _context: McpServerContributionContext<'a, Config>,
    ) -> atlas_engine_extension_api::ExtensionFuture<'a, Vec<McpServerContribution>> {
        Box::pin(async move {
            vec![McpServerContribution::Remove {
                name: ATLAS_APPS_MCP_SERVER_NAME.to_string(),
            }]
        })
    }
}
