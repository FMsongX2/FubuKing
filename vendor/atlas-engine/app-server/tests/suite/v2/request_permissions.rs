// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_final_assistant_message_sse_response;
use app_test_support::create_mock_responses_server_sequence;
use app_test_support::create_request_permissions_sse_response;
use atlas_engine_app_server_protocol::ClientRequest;
use atlas_engine_app_server_protocol::JSONRPCMessage;
use atlas_engine_app_server_protocol::PermissionGrantScope;
use atlas_engine_app_server_protocol::PermissionsRequestApprovalResponse;
use atlas_engine_app_server_protocol::ServerRequest;
use atlas_engine_app_server_protocol::ServerRequestResolvedNotification;
use atlas_engine_app_server_protocol::ThreadStartParams;
use atlas_engine_app_server_protocol::ThreadStartResponse;
use atlas_engine_app_server_protocol::TurnStartParams;
use atlas_engine_app_server_protocol::TurnStartResponse;
use atlas_engine_app_server_protocol::UserInput as V2UserInput;
use atlas_engine_features::Feature;
use core_test_support::skip_if_wine_exec;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_permissions_round_trip() -> Result<()> {
    // TODO(anp): Remove after tool routing accepts a target-native cwd on a different host OS.
    skip_if_wine_exec!(
        Ok(()),
        "request_permissions currently rejects the target-native Windows cwd on the Linux host"
    );

    let atlas_agent_home = tempfile::TempDir::new()?;
    let responses = vec![
        create_request_permissions_sse_response("call1")?,
        create_final_assistant_message_sse_response("done")?,
    ];
    let server = create_mock_responses_server_sequence(responses).await;
    MockResponsesConfig::new(&server.uri())
        .with_approval_policy("untrusted")
        .enable_feature(Feature::RequestPermissionsTool)
        .write(atlas_agent_home.path())?;

    let mut mcp = TestAppServer::builder()
        .with_atlas_agent_home(atlas_agent_home.path())
        .build_initialized()
        .await?;

    let ThreadStartResponse { thread, .. } = mcp
        .start_thread(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;

    let TurnStartResponse { turn, .. } = mcp
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: thread.id.clone(),
                client_user_message_id: None,
                input: vec![V2UserInput::Text {
                    text: "pick a directory".to_string(),
                    text_elements: Vec::new(),
                }],
                model: Some("mock-model".to_string()),
                ..Default::default()
            },
        })
        .await?;

    let server_req = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_request_message(),
    )
    .await??;
    let ServerRequest::PermissionsRequestApproval { request_id, params } = server_req else {
        panic!("expected PermissionsRequestApproval request, got: {server_req:?}");
    };

    assert_eq!(params.thread_id, thread.id);
    assert_eq!(params.turn_id, turn.id);
    assert_eq!(params.item_id, "call1");
    assert!(params.cwd.as_path().is_absolute());
    assert_eq!(params.reason, Some("Select a workspace root".to_string()));
    let requested_file_system = params
        .permissions
        .file_system
        .expect("request should include file system permissions");
    let requested_writes = requested_file_system
        .write
        .clone()
        .expect("request should include write permissions");
    assert_eq!(requested_writes.len(), 2);
    assert_eq!(
        requested_file_system.entries,
        Some(vec![
            atlas_engine_app_server_protocol::FileSystemSandboxEntry {
                path: atlas_engine_app_server_protocol::FileSystemPath::Path {
                    path: requested_writes[0].clone(),
                },
                access: atlas_engine_app_server_protocol::FileSystemAccessMode::Write,
            },
            atlas_engine_app_server_protocol::FileSystemSandboxEntry {
                path: atlas_engine_app_server_protocol::FileSystemPath::Path {
                    path: requested_writes[1].clone(),
                },
                access: atlas_engine_app_server_protocol::FileSystemAccessMode::Write,
            },
        ])
    );
    let resolved_request_id = request_id.clone();

    mcp.send_response(
        request_id,
        serde_json::to_value(PermissionsRequestApprovalResponse {
            permissions: atlas_engine_app_server_protocol::GrantedPermissionProfile {
                network: None,
                file_system: Some(atlas_engine_app_server_protocol::AdditionalFileSystemPermissions {
                    read: None,
                    write: Some(vec![requested_writes[0].clone()]),
                    glob_scan_max_depth: None,
                    entries: None,
                }),
            },
            scope: PermissionGrantScope::Turn,
            strict_auto_review: None,
        })?,
    )
    .await?;

    let mut saw_resolved = false;
    loop {
        let message = timeout(DEFAULT_READ_TIMEOUT, mcp.read_next_message()).await??;
        let JSONRPCMessage::Notification(notification) = message else {
            continue;
        };
        match notification.method.as_str() {
            "serverRequest/resolved" => {
                let resolved: ServerRequestResolvedNotification = serde_json::from_value(
                    notification
                        .params
                        .clone()
                        .expect("serverRequest/resolved params"),
                )?;
                assert_eq!(resolved.thread_id, thread.id);
                assert_eq!(resolved.request_id, resolved_request_id);
                saw_resolved = true;
            }
            "turn/completed" => {
                assert!(saw_resolved, "serverRequest/resolved should arrive first");
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
