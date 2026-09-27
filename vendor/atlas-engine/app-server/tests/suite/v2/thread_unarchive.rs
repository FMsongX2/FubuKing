// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::to_response;
use atlas_engine_app_server::in_process;
use atlas_engine_app_server::in_process::InProcessStartArgs;
use atlas_engine_app_server_protocol::ClientInfo;
use atlas_engine_app_server_protocol::ClientRequest;
use atlas_engine_app_server_protocol::InitializeCapabilities;
use atlas_engine_app_server_protocol::InitializeParams;
use atlas_engine_app_server_protocol::JSONRPCResponse;
use atlas_engine_app_server_protocol::RequestId;
use atlas_engine_app_server_protocol::ThreadArchiveParams;
use atlas_engine_app_server_protocol::ThreadArchiveResponse;
use atlas_engine_app_server_protocol::ThreadReadParams;
use atlas_engine_app_server_protocol::ThreadReadResponse;
use atlas_engine_app_server_protocol::ThreadSection;
use atlas_engine_app_server_protocol::ThreadSectionMoveParams;
use atlas_engine_app_server_protocol::ThreadSectionMoveResponse;
use atlas_engine_app_server_protocol::ThreadStartParams;
use atlas_engine_app_server_protocol::ThreadStartResponse;
use atlas_engine_app_server_protocol::ThreadStatus;
use atlas_engine_app_server_protocol::ThreadUnarchiveParams;
use atlas_engine_app_server_protocol::ThreadUnarchiveResponse;
use atlas_engine_app_server_protocol::ThreadUnarchivedNotification;
use atlas_engine_app_server_protocol::TurnStartParams;
use atlas_engine_app_server_protocol::TurnStartResponse;
use atlas_engine_app_server_protocol::UserInput;
use atlas_engine_arg0::Arg0DispatchPaths;
use atlas_engine_config::CloudConfigBundleLoader;
use atlas_engine_config::LoaderOverrides;
use atlas_engine_core::config::ConfigBuilder;
use atlas_engine_core::find_archived_thread_path_by_id_str;
use atlas_engine_core::find_thread_path_by_id_str;
use atlas_engine_exec_server::EnvironmentManager;
use atlas_engine_feedback::AtlasEngineFeedback;
use atlas_engine_protocol::ThreadId;
use atlas_engine_protocol::models::BaseInstructions;
use atlas_engine_protocol::protocol::SessionSource;
use atlas_engine_protocol::protocol::ThreadMemoryMode;
use atlas_engine_state::PINNED_THREAD_SECTION_ID;
use atlas_engine_state::PINNED_THREAD_SECTION_NAME;
use atlas_engine_thread_store::CreateThreadParams;
use atlas_engine_thread_store::InMemoryThreadStore;
use atlas_engine_thread_store::ThreadMetadataPatch;
use atlas_engine_thread_store::ThreadPersistenceMetadata;
use atlas_engine_thread_store::ThreadStore;
use atlas_engine_thread_store::UpdateThreadMetadataParams;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::fs::FileTimes;
use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use tempfile::TempDir;
use tokio::time::timeout;
use uuid::Uuid;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[tokio::test]
async fn thread_unarchive_moves_rollout_back_into_sessions_directory() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let atlas_agent_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(atlas_agent_home.path())?;

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .build_initialized_with_timeout(DEFAULT_READ_TIMEOUT)
        .await?;

    let start_id = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let ThreadStartResponse { thread, .. } =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(start_id)).await??;

    let rollout_path = thread.path.clone().expect("thread path");

    let turn_start_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![UserInput::Text {
                text: "materialize".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let _: TurnStartResponse =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(turn_start_id)).await??;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let pinned_section = ThreadSection {
        id: PINNED_THREAD_SECTION_ID.to_string(),
        name: PINNED_THREAD_SECTION_NAME.to_string(),
        appearance: None,
    };
    let pin_id = mcp
        .send_thread_section_move_request(ThreadSectionMoveParams {
            thread_id: thread.id.clone(),
            section_id: Some(PINNED_THREAD_SECTION_ID.to_string()),
            before_thread_id: None,
        })
        .await?;
    let _: ThreadSectionMoveResponse =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(pin_id)).await??;
    let read_id = mcp
        .send_thread_read_request(ThreadReadParams {
            thread_id: thread.id.clone(),
            include_turns: false,
        })
        .await?;
    let ThreadReadResponse {
        thread: pinned_thread,
    } = timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(read_id)).await??;
    assert_eq!(pinned_thread.section, Some(pinned_section.clone()));
    let pinned_entered_at = pinned_thread
        .section_entered_at
        .expect("pinned thread should have a section entry timestamp");

    let found_rollout_path =
        find_thread_path_by_id_str(atlas_agent_home.path(), &thread.id, /*state_db_ctx*/ None)
            .await?
            .expect("expected rollout path for thread id to exist");
    assert_paths_match_on_disk(&found_rollout_path, &rollout_path)?;

    let archive_id = mcp
        .send_thread_archive_request(ThreadArchiveParams {
            thread_id: thread.id.clone(),
        })
        .await?;
    let _: ThreadArchiveResponse =
        timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(archive_id)).await??;

    let archived_path = find_archived_thread_path_by_id_str(
        atlas_agent_home.path(),
        &thread.id,
        /*state_db_ctx*/ None,
    )
    .await?
    .expect("expected archived rollout path for thread id to exist");
    let archived_path_display = archived_path.display();
    assert!(
        archived_path.exists(),
        "expected {archived_path_display} to exist"
    );
    let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    let old_timestamp = old_time
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("old timestamp")
        .as_secs() as i64;
    let times = FileTimes::new().set_modified(old_time);
    OpenOptions::new()
        .append(true)
        .open(&archived_path)?
        .set_times(times)?;

    let unarchive_id = mcp
        .send_thread_unarchive_request(ThreadUnarchiveParams {
            thread_id: thread.id.clone(),
        })
        .await?;
    let unarchive_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(unarchive_id)),
    )
    .await??;
    let unarchive_result = unarchive_resp.result.clone();
    let ThreadUnarchiveResponse {
        thread: unarchived_thread,
    } = to_response::<ThreadUnarchiveResponse>(unarchive_resp)?;
    let unarchived_notification: ThreadUnarchivedNotification = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_notification("thread/unarchived"),
    )
    .await??;
    assert_eq!(unarchived_notification.thread_id, thread.id);
    assert_eq!(unarchived_thread.section, Some(pinned_section.clone()));
    assert_eq!(
        unarchived_thread.section_entered_at,
        Some(pinned_entered_at)
    );
    assert!(
        unarchived_thread.updated_at > old_timestamp,
        "expected updated_at to be bumped on unarchive"
    );
    assert_eq!(unarchived_thread.status, ThreadStatus::NotLoaded);

    // Wire contract: thread title field is `name`, serialized as null when unset.
    let thread_json = unarchive_result
        .get("thread")
        .and_then(Value::as_object)
        .expect("thread/unarchive result.thread must be an object");
    assert_eq!(unarchived_thread.name, None);
    assert_eq!(
        thread_json.get("section"),
        Some(&serde_json::to_value(&pinned_section)?)
    );
    assert_eq!(
        thread_json.get("sectionEnteredAt"),
        Some(&Value::from(pinned_entered_at))
    );
    assert_eq!(
        thread_json.get("name"),
        Some(&Value::Null),
        "thread/unarchive must serialize `name: null` when unset"
    );

    let rollout_path_display = rollout_path.display();
    assert!(
        rollout_path.exists(),
        "expected rollout path {rollout_path_display} to be restored"
    );
    assert!(
        !archived_path.exists(),
        "expected archived rollout path {archived_path_display} to be moved"
    );

    Ok(())
}

#[tokio::test]
async fn thread_unarchive_preserves_pathless_store_metadata() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    let store_id = Uuid::new_v4().to_string();
    MockResponsesConfig::new("http://127.0.0.1:1")
        .with_root_config(&format!(
            r#"experimental_thread_store = {{ type = "in_memory", id = "{store_id}" }}"#
        ))
        .write(atlas_agent_home.path())?;
    let store = InMemoryThreadStore::for_id(store_id.clone());
    let _in_memory_store = InMemoryThreadStoreId { store_id };
    let thread_id = ThreadId::from_string("00000000-0000-4000-8000-000000000126")?;
    let parent_thread_id = ThreadId::from_string("00000000-0000-4000-8000-000000000127")?;
    store
        .create_thread(CreateThreadParams {
            session_id: thread_id.into(),
            thread_id,
            extra_config: None,
            forked_from_id: Some(parent_thread_id),
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
    store
        .update_thread_metadata(UpdateThreadMetadataParams {
            thread_id,
            patch: ThreadMetadataPatch {
                name: Some(Some("named pathless thread".to_string())),
                ..Default::default()
            },
            include_archived: true,
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
        .request(ClientRequest::ThreadUnarchive {
            request_id: RequestId::Integer(1),
            params: ThreadUnarchiveParams {
                thread_id: thread_id.to_string(),
            },
        })
        .await?
        .expect("thread/unarchive should succeed");
    let ThreadUnarchiveResponse { thread } = serde_json::from_value(result)?;

    assert_eq!(thread.id, thread_id.to_string());
    assert_eq!(thread.path, None);
    assert_eq!(thread.forked_from_id, Some(parent_thread_id.to_string()));
    assert_eq!(thread.name, Some("named pathless thread".to_string()));

    client.shutdown().await?;
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

fn assert_paths_match_on_disk(actual: &Path, expected: &Path) -> std::io::Result<()> {
    let actual = actual.canonicalize()?;
    let expected = expected.canonicalize()?;
    assert_eq!(actual, expected);
    Ok(())
}
