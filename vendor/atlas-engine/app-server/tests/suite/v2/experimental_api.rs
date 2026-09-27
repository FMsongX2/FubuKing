// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use anyhow::Result;
use app_test_support::DEFAULT_CLIENT_NAME;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_sequence_unchecked;
use app_test_support::to_response;
use atlas_engine_app_server_protocol::AskForApproval;
use atlas_engine_app_server_protocol::ClientInfo;
use atlas_engine_app_server_protocol::InitializeCapabilities;
use atlas_engine_app_server_protocol::JSONRPCError;
use atlas_engine_app_server_protocol::JSONRPCMessage;
use atlas_engine_app_server_protocol::JSONRPCResponse;
use atlas_engine_app_server_protocol::MockExperimentalMethodParams;
use atlas_engine_app_server_protocol::RequestId;
use atlas_engine_app_server_protocol::ThreadMemoryMode;
use atlas_engine_app_server_protocol::ThreadMemoryModeSetParams;
use atlas_engine_app_server_protocol::ThreadRealtimeStartParams;
use atlas_engine_app_server_protocol::ThreadRealtimeStartTransport;
use atlas_engine_app_server_protocol::ThreadSettingsUpdateParams;
use atlas_engine_app_server_protocol::ThreadStartParams;
use atlas_engine_app_server_protocol::ThreadStartResponse;
use atlas_engine_protocol::protocol::RealtimeOutputModality;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn mock_experimental_method_requires_experimental_api_capability() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build()
        .await?;

    let init = mcp
        .initialize_with_capabilities(
            default_client_info(),
            Some(InitializeCapabilities {
                experimental_api: false,
                request_attestation: false,
                opt_out_notification_methods: None,
                mcp_server_openai_form_elicitation: false,
                extensions: None,
            }),
        )
        .await?;
    let JSONRPCMessage::Response(_) = init else {
        anyhow::bail!("expected initialize response, got {init:?}");
    };

    let request_id = mcp
        .send_mock_experimental_method_request(MockExperimentalMethodParams::default())
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_experimental_capability_error(error, "mock/experimentalMethod");
    Ok(())
}

#[tokio::test]
async fn realtime_conversation_start_requires_experimental_api_capability() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build()
        .await?;

    let init = mcp
        .initialize_with_capabilities(
            default_client_info(),
            Some(InitializeCapabilities {
                experimental_api: false,
                request_attestation: false,
                opt_out_notification_methods: None,
                mcp_server_openai_form_elicitation: false,
                extensions: None,
            }),
        )
        .await?;
    let JSONRPCMessage::Response(_) = init else {
        anyhow::bail!("expected initialize response, got {init:?}");
    };

    let request_id = mcp
        .send_thread_realtime_start_request(ThreadRealtimeStartParams {
            client_managed_handoffs: None,
            delegation_ack_filler: None,
            flush_transcript_tail_on_session_end: None,
            atlas_engine_responses_as_items: None,
            atlas_engine_response_item_prefix: None,
            atlas_engine_response_handoff_mode: None,
            atlas_engine_response_handoff_channel_prefixes: None,
            thread_id: "thr_123".to_string(),
            model: None,
            output_modality: RealtimeOutputModality::Audio,
            include_startup_context: None,
            initial_items: None,
            realtime_start_instructions: None,
            realtime_end_instructions: None,
            prompt: Some(Some("hello".to_string())),
            realtime_session_id: None,
            transport: None,
            version: None,
            voice: None,
        })
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_experimental_capability_error(error, "thread/realtime/start");
    Ok(())
}

#[tokio::test]
async fn thread_memory_mode_set_requires_experimental_api_capability() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build()
        .await?;

    let init = mcp
        .initialize_with_capabilities(
            default_client_info(),
            Some(InitializeCapabilities {
                experimental_api: false,
                request_attestation: false,
                opt_out_notification_methods: None,
                mcp_server_openai_form_elicitation: false,
                extensions: None,
            }),
        )
        .await?;
    let JSONRPCMessage::Response(_) = init else {
        anyhow::bail!("expected initialize response, got {init:?}");
    };

    let request_id = mcp
        .send_thread_memory_mode_set_request(ThreadMemoryModeSetParams {
            thread_id: "thr_123".to_string(),
            mode: ThreadMemoryMode::Disabled,
        })
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_experimental_capability_error(error, "thread/memoryMode/set");
    Ok(())
}

#[tokio::test]
async fn thread_settings_update_requires_experimental_api_capability() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build()
        .await?;

    let init = mcp
        .initialize_with_capabilities(
            default_client_info(),
            Some(InitializeCapabilities {
                experimental_api: false,
                request_attestation: false,
                opt_out_notification_methods: None,
                mcp_server_openai_form_elicitation: false,
                extensions: None,
            }),
        )
        .await?;
    let JSONRPCMessage::Response(_) = init else {
        anyhow::bail!("expected initialize response, got {init:?}");
    };

    let request_id = mcp
        .send_thread_settings_update_request(ThreadSettingsUpdateParams {
            thread_id: "thr_123".to_string(),
            ..Default::default()
        })
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_experimental_capability_error(error, "thread/settings/update");
    Ok(())
}

#[tokio::test]
async fn realtime_webrtc_start_requires_experimental_api_capability() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .without_auto_env()
        .build()
        .await?;

    let init = mcp
        .initialize_with_capabilities(
            default_client_info(),
            Some(InitializeCapabilities {
                experimental_api: false,
                request_attestation: false,
                opt_out_notification_methods: None,
                mcp_server_openai_form_elicitation: false,
                extensions: None,
            }),
        )
        .await?;
    let JSONRPCMessage::Response(_) = init else {
        anyhow::bail!("expected initialize response, got {init:?}");
    };

    let request_id = mcp
        .send_thread_realtime_start_request(ThreadRealtimeStartParams {
            client_managed_handoffs: None,
            delegation_ack_filler: None,
            flush_transcript_tail_on_session_end: None,
            atlas_engine_responses_as_items: None,
            atlas_engine_response_item_prefix: None,
            atlas_engine_response_handoff_mode: None,
            atlas_engine_response_handoff_channel_prefixes: None,
            thread_id: "thr_123".to_string(),
            model: None,
            output_modality: RealtimeOutputModality::Audio,
            include_startup_context: None,
            initial_items: None,
            realtime_start_instructions: None,
            realtime_end_instructions: None,
            prompt: Some(Some("hello".to_string())),
            realtime_session_id: None,
            transport: Some(ThreadRealtimeStartTransport::Webrtc {
                sdp: "v=offer\r\n".to_string(),
            }),
            version: None,
            voice: None,
        })
        .await?;
    let error = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_experimental_capability_error(error, "thread/realtime/start");
    Ok(())
}

#[tokio::test]
async fn thread_start_mock_field_requires_experimental_api_capability() -> Result<()> {
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let atlas_agent_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(atlas_agent_home.path())?;

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .build()
        .await?;
    let init = mcp
        .initialize_with_capabilities(
            default_client_info(),
            Some(InitializeCapabilities {
                experimental_api: false,
                request_attestation: false,
                opt_out_notification_methods: None,
                mcp_server_openai_form_elicitation: false,
                extensions: None,
            }),
        )
        .await?;
    let JSONRPCMessage::Response(_) = init else {
        anyhow::bail!("expected initialize response, got {init:?}");
    };

    let request_id = mcp
        .send_thread_start_request(ThreadStartParams {
            mock_experimental_field: Some("mock".to_string()),
            ..Default::default()
        })
        .await?;

    let error = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_experimental_capability_error(error, "thread/start.mockExperimentalField");
    Ok(())
}

#[tokio::test]
async fn thread_start_without_dynamic_tools_allows_without_experimental_api_capability()
-> Result<()> {
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let atlas_agent_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(atlas_agent_home.path())?;

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .build()
        .await?;
    let init = mcp
        .initialize_with_capabilities(
            default_client_info(),
            Some(InitializeCapabilities {
                experimental_api: false,
                request_attestation: false,
                opt_out_notification_methods: None,
                mcp_server_openai_form_elicitation: false,
                extensions: None,
            }),
        )
        .await?;
    let JSONRPCMessage::Response(_) = init else {
        anyhow::bail!("expected initialize response, got {init:?}");
    };

    let request_id = mcp
        .send_thread_start_request(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let _: ThreadStartResponse = to_response(response)?;
    Ok(())
}

#[tokio::test]
async fn thread_start_granular_approval_policy_requires_experimental_api_capability() -> Result<()>
{
    let server = create_mock_responses_server_sequence_unchecked(Vec::new()).await;
    let atlas_agent_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(atlas_agent_home.path())?;

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .build()
        .await?;
    let init = mcp
        .initialize_with_capabilities(
            default_client_info(),
            Some(InitializeCapabilities {
                experimental_api: false,
                request_attestation: false,
                opt_out_notification_methods: None,
                mcp_server_openai_form_elicitation: false,
                extensions: None,
            }),
        )
        .await?;
    let JSONRPCMessage::Response(_) = init else {
        anyhow::bail!("expected initialize response, got {init:?}");
    };

    let request_id = mcp
        .send_thread_start_request(ThreadStartParams {
            approval_policy: Some(AskForApproval::Granular {
                sandbox_approval: true,
                rules: false,
                skill_approval: false,
                request_permissions: true,
                mcp_elicitations: false,
            }),
            ..Default::default()
        })
        .await?;

    let error = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_experimental_capability_error(error, "askForApproval.granular");
    Ok(())
}

fn default_client_info() -> ClientInfo {
    ClientInfo {
        name: DEFAULT_CLIENT_NAME.to_string(),
        title: None,
        version: "0.1.0".to_string(),
    }
}

fn assert_experimental_capability_error(error: JSONRPCError, reason: &str) {
    assert_eq!(error.error.code, -32600);
    assert_eq!(
        error.error.message,
        format!("{reason} requires experimentalApi capability")
    );
    assert_eq!(error.error.data, None);
}
