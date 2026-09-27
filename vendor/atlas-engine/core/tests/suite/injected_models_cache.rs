// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_core::TurnInputRequest;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Result;
use chrono::Utc;
use atlas_engine_http_client::HttpClientFactory;
use atlas_engine_login::AuthManager;
use atlas_engine_login::AtlasEngineAuth;
use atlas_engine_models_manager::cache::ModelsCache;
use atlas_engine_models_manager::cache::ModelsCacheEntry;
use atlas_engine_models_manager::cache::ModelsCacheError;
use atlas_engine_models_manager::cache::ModelsCacheFuture;
use atlas_engine_models_manager::manager::ModelsEndpointClient;
use atlas_engine_models_manager::manager::ModelsEndpointFuture;
use atlas_engine_models_manager::manager::OpenAiModelsManager;
use atlas_engine_models_manager::manager::RefreshStrategy;
use atlas_engine_models_manager::manager::SharedModelsManager;
use atlas_engine_models_manager::model_info::model_info_from_slug;
use atlas_engine_protocol::error::Result as CoreResult;
use atlas_engine_protocol::openai_models::ModelInfo;
use atlas_engine_protocol::openai_models::ModelVisibility;
use atlas_engine_protocol::protocol::EventMsg;
use atlas_engine_protocol::protocol::ThreadSettingsOverrides;
use atlas_engine_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::submit_thread_settings;
use core_test_support::test_atlas_engine::test_atlas_engine;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;

#[derive(Debug)]
struct TestModelsCache {
    entry: Option<ModelsCacheEntry>,
    load_error: bool,
    stored_entries: Mutex<Vec<ModelsCacheEntry>>,
}

impl ModelsCache for TestModelsCache {
    fn load<'a>(
        &'a self,
        _client_version: &'a str,
    ) -> ModelsCacheFuture<'a, Result<Option<ModelsCacheEntry>, ModelsCacheError>> {
        Box::pin(async move {
            if self.load_error {
                Err(ModelsCacheError::new("test load failure"))
            } else {
                Ok(self.entry.clone())
            }
        })
    }

    fn store<'a>(
        &'a self,
        entry: &'a ModelsCacheEntry,
    ) -> ModelsCacheFuture<'a, Result<(), ModelsCacheError>> {
        Box::pin(async move {
            self.stored_entries
                .lock()
                .expect("stored entries lock should not be poisoned")
                .push(entry.clone());
            Ok(())
        })
    }

    fn refresh_ttl<'a>(
        &'a self,
        _client_version: &'a str,
    ) -> ModelsCacheFuture<'a, Result<(), ModelsCacheError>> {
        Box::pin(async move {
            let mut entry = self
                .entry
                .clone()
                .ok_or_else(|| ModelsCacheError::new("cache not found"))?;
            entry.fetched_at = Utc::now();
            self.stored_entries
                .lock()
                .expect("stored entries lock should not be poisoned")
                .push(entry);
            Ok(())
        })
    }
}

#[derive(Debug)]
struct TestModelsEndpoint {
    models: Vec<ModelInfo>,
    fetch_count: AtomicUsize,
}

impl TestModelsEndpoint {
    fn new(models: Vec<ModelInfo>) -> Arc<Self> {
        Arc::new(Self {
            models,
            fetch_count: AtomicUsize::new(0),
        })
    }
}

impl ModelsEndpointClient for TestModelsEndpoint {
    fn has_command_auth(&self) -> bool {
        false
    }

    fn uses_atlas_engine_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(async { true })
    }

    fn list_models<'a>(
        &'a self,
        _client_version: &'a str,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<(Vec<ModelInfo>, Option<String>)>> {
        Box::pin(async move {
            self.fetch_count.fetch_add(1, Ordering::SeqCst);
            Ok((self.models.clone(), None))
        })
    }
}

fn remote_model(slug: &str) -> ModelInfo {
    ModelInfo {
        visibility: ModelVisibility::List,
        used_fallback_model_metadata: false,
        ..model_info_from_slug(slug)
    }
}

fn models_manager(
    cache: Arc<dyn ModelsCache>,
    endpoint: Arc<TestModelsEndpoint>,
) -> SharedModelsManager {
    Arc::new(OpenAiModelsManager::new_with_cache(
        cache,
        endpoint,
        Some(AuthManager::from_auth_for_testing(
            AtlasEngineAuth::create_dummy_chatgpt_auth_for_testing(),
        )),
    ))
}

async fn run_agent_with_model(models_manager: SharedModelsManager, model_slug: &str) -> Result<()> {
    let server = responses::start_mock_server().await;
    let response_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    let mut builder = test_atlas_engine()
        .with_auth(AtlasEngineAuth::create_dummy_chatgpt_auth_for_testing())
        .with_models_manager(models_manager);
    let test = builder.build(&server).await?;
    let available_models = test
        .thread_manager
        .get_models_manager()
        .list_models(
            RefreshStrategy::OnlineIfUncached,
            atlas_engine_core::test_support::default_http_client_factory(),
        )
        .await;
    assert!(
        available_models
            .iter()
            .any(|model| model.model == model_slug)
    );

    submit_thread_settings(
        &test.atlas_engine,
        ThreadSettingsOverrides {
            model: Some(model_slug.to_string()),
            ..Default::default()
        },
    )
    .await?;
    test.atlas_engine
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    loop {
        if matches!(
            wait_for_event(&test.atlas_engine, |_| true).await,
            EventMsg::TurnComplete(_)
        ) {
            break;
        }
    }

    assert_eq!(
        response_mock.single_request().body_json()["model"],
        model_slug
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn injected_cache_hit_drives_agent_model_selection() -> Result<()> {
    let model_slug = "injected-cache-model";
    let cache = Arc::new(TestModelsCache {
        entry: Some(ModelsCacheEntry {
            fetched_at: Utc::now(),
            etag: None,
            client_version: Some(atlas_engine_models_manager::client_version_to_whole()),
            models: vec![remote_model(model_slug)],
        }),
        load_error: false,
        stored_entries: Mutex::new(Vec::new()),
    });
    let endpoint = TestModelsEndpoint::new(Vec::new());

    run_agent_with_model(models_manager(cache, endpoint.clone()), model_slug).await?;

    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn injected_cache_error_falls_back_for_agent_model_selection() -> Result<()> {
    let model_slug = "injected-cache-fallback-model";
    let cache = Arc::new(TestModelsCache {
        entry: None,
        load_error: true,
        stored_entries: Mutex::new(Vec::new()),
    });
    let endpoint = TestModelsEndpoint::new(vec![remote_model(model_slug)]);

    run_agent_with_model(models_manager(cache.clone(), endpoint.clone()), model_slug).await?;

    assert!(endpoint.fetch_count.load(Ordering::SeqCst) >= 1);
    assert!(
        !cache
            .stored_entries
            .lock()
            .expect("stored entries lock should not be poisoned")
            .is_empty()
    );
    Ok(())
}
