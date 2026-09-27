// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use crate::config_manager::ConfigManager;
use atlas_engine_core::AtlasEngineThread;
use atlas_engine_core::ThreadManager;
use atlas_engine_core::config::Config;
use std::io;
use std::sync::Arc;
use tracing::warn;

pub(crate) async fn reload_mcp_config(
    thread_manager: &Arc<ThreadManager>,
    config_manager: &ConfigManager,
) -> io::Result<()> {
    config_manager
        .load_latest_config(/*fallback_cwd*/ None)
        .await?;
    let mut refreshes = Vec::new();
    for thread_id in thread_manager.list_thread_ids().await {
        let thread = thread_manager
            .get_thread(thread_id)
            .await
            .map_err(|err| io::Error::other(format!("failed to load thread {thread_id}: {err}")))?;
        let config = load_refresh_config(thread.as_ref(), config_manager).await?;
        refreshes.push((thread, config));
    }
    for (thread, config) in refreshes {
        thread.refresh_mcp_config(config).await;
    }
    Ok(())
}

pub(crate) async fn reload_mcp_config_best_effort(
    thread_manager: &Arc<ThreadManager>,
    config_manager: &ConfigManager,
) {
    for thread_id in thread_manager.list_thread_ids().await {
        let thread = match thread_manager.get_thread(thread_id).await {
            Ok(thread) => thread,
            Err(err) => {
                warn!(%thread_id, %err, "failed to load thread for MCP configuration refresh");
                continue;
            }
        };
        let config = match load_refresh_config(thread.as_ref(), config_manager).await {
            Ok(config) => config,
            Err(err) => {
                warn!(%thread_id, %err, "failed to load thread MCP configuration");
                continue;
            }
        };
        thread.refresh_mcp_config(config).await;
    }
}

async fn load_refresh_config(
    thread: &AtlasEngineThread,
    config_manager: &ConfigManager,
) -> io::Result<Config> {
    let thread_config = thread.config().await;
    config_manager
        .load_latest_config_for_thread(thread_config.as_ref())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::ThreadExtensionDependencies;
    use crate::extensions::guardian_agent_spawner;
    use crate::extensions::thread_extensions;
    use atlas_engine_arg0::Arg0DispatchPaths;
    use atlas_engine_config::CloudConfigBundleLoader;
    use atlas_engine_config::LoaderOverrides;
    use atlas_engine_config::ThreadConfigContext;
    use atlas_engine_config::ThreadConfigLoadError;
    use atlas_engine_config::ThreadConfigLoadErrorCode;
    use atlas_engine_config::ThreadConfigLoader;
    use atlas_engine_config::ThreadConfigSource;
    use atlas_engine_config::types::AuthKeyringBackendKind;
    use atlas_engine_config::types::McpServerConfig;
    use atlas_engine_core::config::ConfigOverrides;
    use atlas_engine_core::init_state_db;
    use atlas_engine_core::thread_store_from_config;
    use atlas_engine_exec_server::EnvironmentManager;
    use atlas_engine_extension_api::NoopExtensionEventSink;
    use atlas_engine_home::AtlasAgentHomeUserInstructionsProvider;
    use atlas_engine_login::AuthManager;
    use atlas_engine_login::AtlasEngineAuth;
    use atlas_engine_protocol::protocol::SessionSource;
    use atlas_engine_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use tempfile::TempDir;

    #[tokio::test]
    async fn strict_refresh_reports_thread_planning_failures() -> anyhow::Result<()> {
        let (temp_dir, thread_manager, config_manager, _loader) = refresh_test_state().await?;
        std::fs::write(
            temp_dir.path().join(atlas_engine_config::CONFIG_TOML_FILE),
            "[features]\nsecret_auth_storage = true\n",
        )?;

        let err = reload_mcp_config(&thread_manager, &config_manager)
            .await
            .expect_err("strict refresh should fail");

        assert_eq!(err.to_string(), "failed to load refresh config");
        for thread_id in thread_manager.list_thread_ids().await {
            assert_eq!(
                thread_manager
                    .get_thread(thread_id)
                    .await?
                    .config()
                    .await
                    .auth_keyring_backend_kind(),
                AuthKeyringBackendKind::Direct
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn best_effort_refresh_updates_healthy_threads() -> anyhow::Result<()> {
        let (temp_dir, thread_manager, config_manager, loader) = refresh_test_state().await?;
        std::fs::write(
            temp_dir.path().join(atlas_engine_config::CONFIG_TOML_FILE),
            "[features]\nsecret_auth_storage = true\n",
        )?;

        reload_mcp_config_best_effort(&thread_manager, &config_manager).await;

        assert_eq!(loader.good_loads.load(Ordering::Relaxed), 1);
        assert_eq!(loader.bad_loads.load(Ordering::Relaxed), 1);
        for thread_id in thread_manager.list_thread_ids().await {
            let thread = thread_manager.get_thread(thread_id).await?;
            let config = thread.config().await;
            let expected = if config.cwd.ends_with("good") {
                AuthKeyringBackendKind::Secrets
            } else {
                AuthKeyringBackendKind::Direct
            };
            assert_eq!(config.auth_keyring_backend_kind(), expected);
        }
        Ok(())
    }

    #[tokio::test]
    async fn invalidation_does_not_reload_thread_config() -> anyhow::Result<()> {
        let (_temp_dir, thread_manager, _config_manager, loader) = refresh_test_state().await?;

        thread_manager.invalidate_mcp_runtimes().await;

        assert_eq!(loader.good_loads.load(Ordering::Relaxed), 0);
        assert_eq!(loader.bad_loads.load(Ordering::Relaxed), 0);
        Ok(())
    }

    #[tokio::test]
    async fn mcp_config_reload_only_applies_mcp_inputs() -> anyhow::Result<()> {
        let (temp_dir, thread_manager, config_manager, _loader) = refresh_test_state().await?;
        std::fs::write(
            temp_dir.path().join(atlas_engine_config::CONFIG_TOML_FILE),
            "model = \"unrelated-model-change\"\n[features]\nsecret_auth_storage = true\n",
        )?;

        let mut good_thread = None;
        for thread_id in thread_manager.list_thread_ids().await {
            let thread = thread_manager.get_thread(thread_id).await?;
            let thread_config = thread.config().await;
            if thread_config.cwd.ends_with("good") {
                good_thread = Some(thread);
                break;
            }
        }
        let thread = good_thread.expect("good test thread should exist");
        let original_model = thread.config().await.model.clone();

        let refresh_config = load_refresh_config(thread.as_ref(), &config_manager).await?;
        thread.refresh_mcp_config(refresh_config).await;

        assert_eq!(
            thread.config().await.auth_keyring_backend_kind(),
            AuthKeyringBackendKind::Secrets
        );
        assert_eq!(thread.config().await.model, original_model);
        Ok(())
    }

    #[tokio::test]
    async fn refresh_config_preserves_thread_mcp_overrides() -> anyhow::Result<()> {
        let (temp_dir, thread_manager, config_manager, _loader) = refresh_test_state().await?;
        let initial_config_manager =
            ConfigManager::without_managed_config_for_tests(temp_dir.path().to_path_buf());
        let thread_config = initial_config_manager
            .load_for_cwd(
                Some(HashMap::from([
                    (
                        "mcp_servers.thread.command".to_string(),
                        json!("thread-mcp"),
                    ),
                    ("mcp_servers.thread.enabled".to_string(), json!(false)),
                ])),
                ConfigOverrides::default(),
                Some(temp_dir.path().join("good")),
            )
            .await?;
        let thread = thread_manager
            .start_thread(atlas_engine_core::StartThreadOptions::new(thread_config))
            .await?
            .thread;
        std::fs::write(
            temp_dir.path().join(atlas_engine_config::CONFIG_TOML_FILE),
            r#"
[mcp_servers.global]
command = "global-mcp"
enabled = false
"#,
        )?;

        let refresh_config = load_refresh_config(thread.as_ref(), &config_manager).await?;
        let mut actual = refresh_config.mcp_servers.get().clone();
        actual.remove(atlas_engine_mcp::ATLAS_APPS_MCP_SERVER_NAME);
        let expected = serde_json::from_value::<HashMap<String, McpServerConfig>>(json!({
            "global": {
                "command": "global-mcp",
                "enabled": false
            },
            "thread": {
                "command": "thread-mcp",
                "enabled": false
            }
        }))?;

        assert_eq!(actual, expected);
        Ok(())
    }

    #[tokio::test]
    async fn strict_refresh_installs_refreshed_thread_mcp_config() -> anyhow::Result<()> {
        let (temp_dir, thread_manager, config_manager, _loader) = refresh_test_state().await?;
        let mut good_thread = None;
        for thread_id in thread_manager.list_thread_ids().await {
            let thread = thread_manager.get_thread(thread_id).await?;
            let thread_config = thread.config().await;
            if thread_config.cwd.ends_with("good") {
                good_thread = Some(thread);
            } else {
                thread_manager.remove_thread(&thread_id).await;
            }
        }
        let thread = good_thread.expect("good test thread should exist");
        std::fs::write(
            temp_dir.path().join(atlas_engine_config::CONFIG_TOML_FILE),
            r#"
[mcp_servers.refreshed]
command = "refreshed-mcp"
enabled = false
"#,
        )?;

        reload_mcp_config(&thread_manager, &config_manager).await?;

        assert!(
            thread
                .config()
                .await
                .mcp_servers
                .get()
                .contains_key("refreshed")
        );
        Ok(())
    }

    async fn refresh_test_state() -> anyhow::Result<(
        TempDir,
        Arc<ThreadManager>,
        ConfigManager,
        Arc<CountingThreadConfigLoader>,
    )> {
        let temp_dir = TempDir::new()?;
        let good_cwd = temp_dir.path().join("good");
        let bad_cwd = temp_dir.path().join("bad");
        std::fs::create_dir_all(&good_cwd)?;
        std::fs::create_dir_all(&bad_cwd)?;
        std::fs::write(
            temp_dir.path().join(atlas_engine_config::CONFIG_TOML_FILE),
            "[features]\nsecret_auth_storage = false\n",
        )?;

        let initial_config_manager =
            ConfigManager::without_managed_config_for_tests(temp_dir.path().to_path_buf());
        let good_config = initial_config_manager
            .load_for_cwd(
                /*request_overrides*/ None,
                ConfigOverrides::default(),
                Some(good_cwd.clone()),
            )
            .await?;
        let bad_config = initial_config_manager
            .load_for_cwd(
                /*request_overrides*/ None,
                ConfigOverrides::default(),
                Some(bad_cwd.clone()),
            )
            .await?;

        let auth_manager = AuthManager::from_auth_for_testing(AtlasEngineAuth::from_api_key("dummy"));
        let state_db = init_state_db(&good_config)
            .await
            .expect("refresh tests require state db");
        let thread_store = thread_store_from_config(&good_config, Some(state_db.clone()));
        let environment_manager = Arc::new(EnvironmentManager::default_for_tests());
        let executor_skill_provider: Arc<dyn atlas_engine_skills_extension::SkillProvider> = Arc::new(
            atlas_engine_skills_extension::ExecutorSkillProvider::new_with_restriction_product(
                Arc::clone(&environment_manager),
                SessionSource::Exec.restriction_product(),
            ),
        );
        let thread_manager = Arc::new_cyclic(|thread_manager| {
            ThreadManager::new(
                &good_config,
                auth_manager.clone(),
                atlas_engine_core::build_models_manager(&good_config, auth_manager.clone()),
                atlas_engine_core::AtlasAppsToolsCache::default(),
                SessionSource::Exec,
                Arc::clone(&environment_manager),
                thread_extensions(
                    guardian_agent_spawner(thread_manager.clone()),
                    ThreadExtensionDependencies {
                        event_sink: Arc::new(NoopExtensionEventSink),
                        auth_manager: auth_manager.clone(),
                        state_db: Some(state_db.clone()),
                        analytics_events_client: atlas_engine_analytics::AnalyticsEventsClient::disabled(),
                        thread_manager: thread_manager.clone(),
                        goal_service: Arc::new(atlas_engine_goal_extension::GoalService::new()),
                        environment_manager: Arc::clone(&environment_manager),
                        executor_skill_provider: Arc::clone(&executor_skill_provider),
                        git_attribution_base_url: good_config.chatgpt_base_url.clone(),
                        http_client_factory: good_config.http_client_factory(),
                        queue_service: None,
                    },
                ),
                Arc::new(AtlasAgentHomeUserInstructionsProvider::new(
                    good_config.atlas_agent_home.clone(),
                )),
                /*analytics_events_client*/ None,
                Arc::clone(&thread_store),
                atlas_engine_core::local_agent_graph_store_from_state_db(Some(&state_db)),
                "11111111-1111-4111-8111-111111111111".to_string(),
                /*attestation_provider*/ None,
                /*external_time_provider*/ None,
            )
        });
        thread_manager
            .start_thread(atlas_engine_core::StartThreadOptions::new(good_config))
            .await?;
        thread_manager
            .start_thread(atlas_engine_core::StartThreadOptions::new(bad_config))
            .await?;

        let loader = Arc::new(CountingThreadConfigLoader {
            good_cwd: AbsolutePathBuf::try_from(good_cwd)?,
            bad_cwd: AbsolutePathBuf::try_from(bad_cwd)?,
            good_loads: AtomicUsize::new(0),
            bad_loads: AtomicUsize::new(0),
        });
        let config_manager = ConfigManager::new(
            temp_dir.path().to_path_buf(),
            Vec::new(),
            LoaderOverrides::without_managed_config_for_tests(),
            /*strict_config*/ false,
            CloudConfigBundleLoader::default(),
            Arg0DispatchPaths::default(),
            loader.clone(),
        );

        Ok((temp_dir, thread_manager, config_manager, loader))
    }

    struct CountingThreadConfigLoader {
        good_cwd: AbsolutePathBuf,
        bad_cwd: AbsolutePathBuf,
        good_loads: AtomicUsize,
        bad_loads: AtomicUsize,
    }

    impl CountingThreadConfigLoader {
        async fn load(
            &self,
            context: ThreadConfigContext,
        ) -> Result<Vec<ThreadConfigSource>, ThreadConfigLoadError> {
            if context.cwd.as_ref() == Some(&self.good_cwd) {
                self.good_loads.fetch_add(1, Ordering::Relaxed);
            }
            if context.cwd.as_ref() == Some(&self.bad_cwd) {
                self.bad_loads.fetch_add(1, Ordering::Relaxed);
                return Err(ThreadConfigLoadError::new(
                    ThreadConfigLoadErrorCode::Internal,
                    /*status_code*/ None,
                    "failed to load refresh config",
                ));
            }
            Ok(Vec::new())
        }
    }

    impl ThreadConfigLoader for CountingThreadConfigLoader {
        fn load(
            &self,
            context: ThreadConfigContext,
        ) -> atlas_engine_config::ThreadConfigLoaderFuture<'_, Vec<ThreadConfigSource>> {
            Box::pin(CountingThreadConfigLoader::load(self, context))
        }
    }
}
