// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_fake_rollout;
use app_test_support::rollout_path;
use atlas_engine_app_server::in_process;
use atlas_engine_app_server::in_process::InProcessStartArgs;
use atlas_engine_app_server_protocol::ClientInfo;
use atlas_engine_app_server_protocol::ClientRequest;
use atlas_engine_app_server_protocol::ConversationSummary;
use atlas_engine_app_server_protocol::GetConversationSummaryParams;
use atlas_engine_app_server_protocol::GetConversationSummaryResponse;
use atlas_engine_app_server_protocol::InitializeCapabilities;
use atlas_engine_app_server_protocol::InitializeParams;
use atlas_engine_app_server_protocol::RequestId;
use atlas_engine_arg0::Arg0DispatchPaths;
use atlas_engine_config::CloudConfigBundleLoader;
use atlas_engine_config::LoaderOverrides;
use atlas_engine_core::config::ConfigBuilder;
use atlas_engine_exec_server::EnvironmentManager;
use atlas_engine_feedback::AtlasEngineFeedback;
use atlas_engine_protocol::ThreadId;
use atlas_engine_protocol::models::BaseInstructions;
use atlas_engine_protocol::protocol::SessionSource;
use atlas_engine_protocol::protocol::ThreadMemoryMode;
use atlas_engine_thread_store::CreateThreadParams;
use atlas_engine_thread_store::InMemoryThreadStore;
use atlas_engine_thread_store::ThreadPersistenceMetadata;
use atlas_engine_thread_store::ThreadStore;
use atlas_engine_utils_absolute_path::AbsolutePathBuf;
use core_test_support::test_path_buf;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

const FILENAME_TS: &str = "2025-01-02T12-00-00";
const META_RFC3339: &str = "2025-01-02T12:00:00Z";
const CREATED_AT_RFC3339: &str = "2025-01-02T12:00:00.000Z";
const UPDATED_AT_RFC3339: &str = "2025-01-02T12:00:00.000Z";
const PREVIEW: &str = "Summarize this conversation";
const MODEL_PROVIDER: &str = "openai";

fn expected_summary(conversation_id: ThreadId, path: PathBuf) -> ConversationSummary {
    ConversationSummary {
        conversation_id,
        path,
        preview: PREVIEW.to_string(),
        timestamp: Some(CREATED_AT_RFC3339.to_string()),
        updated_at: Some(UPDATED_AT_RFC3339.to_string()),
        model_provider: MODEL_PROVIDER.to_string(),
        cwd: test_path_buf("/"),
        cli_version: "0.0.0".to_string(),
        source: SessionSource::Cli,
        git_info: None,
    }
}

fn normalized_canonical_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    Ok(AbsolutePathBuf::from_absolute_path(path.as_ref().canonicalize()?)?.into_path_buf())
}

fn normalized_summary_path(mut summary: ConversationSummary) -> Result<ConversationSummary> {
    if !summary.path.as_os_str().is_empty() {
        summary.path = normalized_canonical_path(summary.path)?;
    }
    if !summary.cwd.as_os_str().is_empty() {
        summary.cwd = AbsolutePathBuf::from_absolute_path(summary.cwd)?.into_path_buf();
    }
    Ok(summary)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_conversation_summary_by_thread_id_reads_rollout() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    let conversation_id = create_fake_rollout(
        atlas_agent_home.path(),
        FILENAME_TS,
        META_RFC3339,
        PREVIEW,
        Some(MODEL_PROVIDER),
        /*git_info*/ None,
    )?;
    let thread_id = ThreadId::from_string(&conversation_id)?;
    let expected = expected_summary(
        thread_id,
        normalized_canonical_path(rollout_path(
            atlas_agent_home.path(),
            FILENAME_TS,
            &conversation_id,
        ))?,
    );

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;

    let received: GetConversationSummaryResponse = mcp
        .request(|request_id| ClientRequest::GetConversationSummary {
            request_id,
            params: GetConversationSummaryParams::ThreadId {
                conversation_id: thread_id,
            },
        })
        .await?;

    assert_eq!(normalized_summary_path(received.summary)?, expected);
    Ok(())
}

#[tokio::test]
async fn get_conversation_summary_by_thread_id_reads_pathless_store_thread() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    let store_id = Uuid::new_v4().to_string();
    create_config_toml_with_in_memory_thread_store(atlas_agent_home.path(), &store_id)?;
    let store = InMemoryThreadStore::for_id(store_id.clone());
    let _in_memory_store = InMemoryThreadStoreId { store_id };
    let thread_id = ThreadId::from_string("00000000-0000-4000-8000-000000000125")?;
    store
        .create_thread(CreateThreadParams {
            session_id: thread_id.into(),
            thread_id,
            extra_config: None,
            forked_from_id: None,
            parent_thread_id: None,
            source: SessionSource::Cli,
            thread_source: None,
            originator: "test_originator".to_string(),
            base_instructions: BaseInstructions::default(),
            dynamic_tools: Vec::new(),
            selected_capability_roots: Vec::new(),
            multi_agent_version: None,
            history_mode: Default::default(),
            history_base: None,
            subagent_history_start_ordinal: None,
            initial_window_id: Uuid::now_v7().to_string(),
            metadata: ThreadPersistenceMetadata {
                cwd: None,
                model_provider: "test-provider".to_string(),
                memory_mode: ThreadMemoryMode::Disabled,
            },
        })
        .await?;

    let loader_overrides = LoaderOverrides::without_managed_config_for_tests();
    let config = ConfigBuilder::default()
        .atlas_agent_home(atlas_agent_home.path().to_path_buf())
        .fallback_cwd(Some(atlas_agent_home.path().to_path_buf()))
        .loader_overrides(loader_overrides.clone())
        .build()
        .await?;
    let client = in_process::start(InProcessStartArgs {
        arg0_paths: Arg0DispatchPaths::default(),
        config: Arc::new(config),
        cli_overrides: Vec::new(),
        loader_overrides,
        strict_config: false,
        cloud_config_bundle: CloudConfigBundleLoader::default(),
        thread_config_loader: Arc::new(atlas_engine_config::NoopThreadConfigLoader),
        feedback: AtlasEngineFeedback::new(),
        log_db: None,
        state_db: None,
        environment_manager: Arc::new(EnvironmentManager::default_for_tests()),
        config_warnings: Vec::new(),
        session_source: SessionSource::Cli,
        enable_atlas_engine_api_key_env: false,
        initialize: InitializeParams {
            client_info: ClientInfo {
                name: "atlas-engine-app-server-tests".to_string(),
                title: None,
                version: "0.1.0".to_string(),
            },
            capabilities: Some(InitializeCapabilities {
                experimental_api: true,
                ..Default::default()
            }),
        },
        channel_capacity: in_process::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    })
    .await?;

    let result = client
        .request(ClientRequest::GetConversationSummary {
            request_id: RequestId::Integer(1),
            params: GetConversationSummaryParams::ThreadId {
                conversation_id: thread_id,
            },
        })
        .await?
        .expect("getConversationSummary should succeed");
    let GetConversationSummaryResponse { summary } = serde_json::from_value(result)?;

    assert_eq!(summary.conversation_id, thread_id);
    assert_eq!(summary.path, PathBuf::new());
    assert_eq!(summary.cwd, PathBuf::new());
    assert_eq!(summary.model_provider, "test");

    client.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_conversation_summary_by_relative_rollout_path_resolves_from_atlas_agent_home() -> Result<()>
{
    let atlas_agent_home = TempDir::new()?;
    let conversation_id = create_fake_rollout(
        atlas_agent_home.path(),
        FILENAME_TS,
        META_RFC3339,
        PREVIEW,
        Some(MODEL_PROVIDER),
        /*git_info*/ None,
    )?;
    let thread_id = ThreadId::from_string(&conversation_id)?;
    let rollout_path = rollout_path(atlas_agent_home.path(), FILENAME_TS, &conversation_id);
    let relative_path = rollout_path.strip_prefix(atlas_agent_home.path())?.to_path_buf();
    let expected = expected_summary(thread_id, normalized_canonical_path(rollout_path)?);

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;

    let received: GetConversationSummaryResponse = mcp
        .request(|request_id| ClientRequest::GetConversationSummary {
            request_id,
            params: GetConversationSummaryParams::RolloutPath {
                rollout_path: relative_path,
            },
        })
        .await?;

    assert_eq!(normalized_summary_path(received.summary)?, expected);
    Ok(())
}

struct InMemoryThreadStoreId {
    store_id: String,
}

impl Drop for InMemoryThreadStoreId {
    fn drop(&mut self) {
        InMemoryThreadStore::remove_id(&self.store_id);
    }
}

fn create_config_toml_with_in_memory_thread_store(
    atlas_agent_home: &Path,
    store_id: &str,
) -> std::io::Result<()> {
    MockResponsesConfig::new("http://127.0.0.1:1")
        .with_root_config(&format!(
            "experimental_thread_store = {{ type = \"in_memory\", id = \"{store_id}\" }}"
        ))
        .write(atlas_agent_home)
}
