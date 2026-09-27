// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use atlas_engine_app_server_protocol::ThreadHistoryMode;
use atlas_engine_app_server_protocol::ThreadResumeParams;
use atlas_engine_app_server_protocol::ThreadResumeResponse;
use atlas_engine_app_server_protocol::ThreadStartParams;
use atlas_engine_app_server_protocol::ThreadStartResponse;
use atlas_engine_app_server_protocol::TurnStartParams;
use atlas_engine_app_server_protocol::UserInput;
use atlas_engine_thread_store::LocalThreadStore;
use atlas_engine_thread_store::LocalThreadStoreConfig;
use atlas_engine_thread_store::RolloutMigrationMode;
use atlas_engine_thread_store::RolloutMigrationOptions;
use atlas_engine_thread_store::RolloutMigrationStatus;
use atlas_engine_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[tokio::test]
async fn migrated_legacy_thread_cold_resume_preserves_model_context() -> Result<()> {
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-1"),
                responses::ev_assistant_message("msg-1", "legacy assistant message"),
                responses::ev_completed("resp-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-2"),
                responses::ev_assistant_message("msg-2", "resumed assistant message"),
                responses::ev_completed("resp-2"),
            ]),
        ],
    )
    .await;
    let atlas_agent_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(atlas_agent_home.path())?;

    let mut primary = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .build_initialized()
        .await?;
    let start_id = primary
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            history_mode: Some(ThreadHistoryMode::Legacy),
            ..Default::default()
        })
        .await?;
    let ThreadStartResponse { thread, .. } =
        timeout(DEFAULT_READ_TIMEOUT, primary.read_response(start_id)).await??;
    timeout(
        DEFAULT_READ_TIMEOUT,
        primary.start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.id.clone(),
            input: vec![UserInput::Text {
                text: "legacy user message".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        }),
    )
    .await??;
    timeout(DEFAULT_READ_TIMEOUT, primary.shutdown_gracefully()).await??;

    let sqlite = atlas_engine_state::SqliteConfig::new_for_testing(atlas_agent_home.path().abs());
    let state_db =
        atlas_engine_state::StateRuntime::init(sqlite.clone(), "mock_provider".to_string()).await?;
    let store = LocalThreadStore::new(
        LocalThreadStoreConfig {
            atlas_agent_home: atlas_agent_home.path().to_path_buf(),
            sqlite,
            default_model_provider_id: "mock_provider".to_string(),
        },
        Some(state_db),
    );
    let report = store
        .migrate_rollouts(RolloutMigrationOptions {
            mode: RolloutMigrationMode::Apply,
            max_mib_per_second: Some(1024),
            ..RolloutMigrationOptions::default()
        })
        .await?;
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].status, RolloutMigrationStatus::Migrated);
    drop(store);

    let mut secondary = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .build_initialized()
        .await?;
    let resume_id = secondary
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.id.clone(),
            exclude_turns: true,
            ..Default::default()
        })
        .await?;
    let ThreadResumeResponse {
        thread: resumed, ..
    } = timeout(DEFAULT_READ_TIMEOUT, secondary.read_response(resume_id)).await??;
    assert_eq!(resumed.history_mode, ThreadHistoryMode::Paginated);

    timeout(
        DEFAULT_READ_TIMEOUT,
        secondary.start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.id,
            input: vec![UserInput::Text {
                text: "resumed user message".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        }),
    )
    .await??;

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 2);
    let resumed_request = requests.last().expect("resumed turn request");
    let user_messages = resumed_request.message_input_texts("user");
    assert!(user_messages.contains(&"legacy user message".to_string()));
    assert!(user_messages.contains(&"resumed user message".to_string()));
    assert!(resumed_request.body_contains_text("legacy assistant message"));

    Ok(())
}
