// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_core::NotSubmittedReason;
use atlas_engine_core::RecoverTurnRequest;
use atlas_engine_core::StartIfIdleSubmission;
use atlas_engine_core::SteerSubmission;
use atlas_engine_core::TurnInput;
use atlas_engine_core::TurnInputRequest;
use atlas_engine_core::TurnInputSubmission;
use atlas_engine_core::TurnStartOptions;
use atlas_engine_core::config::Constrained;
use atlas_engine_protocol::config_types::CollaborationMode;
use atlas_engine_protocol::config_types::ModeKind;
use atlas_engine_protocol::config_types::Settings;
use atlas_engine_protocol::protocol::AskForApproval;
use atlas_engine_protocol::protocol::EventMsg;
use atlas_engine_protocol::protocol::ThreadSettingsOverrides;
use atlas_engine_protocol::protocol::TurnEnvironmentSelections;
use atlas_engine_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_atlas_engine::local;
use core_test_support::test_atlas_engine::test_atlas_engine;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Barrier;
use tokio::sync::oneshot;
use tokio::time::timeout;

fn user_message_request(text: &str) -> TurnInputRequest {
    TurnInputRequest::user_input(vec![UserInput::Text {
        text: text.to_string(),
        text_elements: Vec::new(),
    }])
}

async fn submit_user_message(
    atlas_engine: &atlas_engine_core::AtlasEngineThread,
    text: &str,
) -> atlas_engine_protocol::error::Result<TurnInputSubmission> {
    atlas_engine.start_or_steer_turn(user_message_request(text)).await
}

#[tokio::test]
async fn start_turn_if_idle_rejects_non_user_input_that_requests_plan_mode() {
    let server = responses::start_mock_server().await;
    let test = test_atlas_engine()
        .build_with_auto_env(&server)
        .await
        .expect("build turn-input submission session");
    let original_collaboration_mode = test.atlas_engine.config_snapshot().await.collaboration_mode;

    let submission = test
        .atlas_engine
        .start_turn_if_idle(
            TurnInputRequest::new(TurnInput::ResponseItem(responses::user_message_item(
                "automatic input",
            )))
            .with_thread_settings(ThreadSettingsOverrides {
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Plan,
                    settings: Settings {
                        model: test.session_configured.model.clone(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            }),
        )
        .await
        .expect("idle turn submission should return a typed rejection");

    assert_eq!(
        submission,
        StartIfIdleSubmission::NotSubmitted {
            reason: NotSubmittedReason::PlanMode,
        }
    );
    assert_eq!(
        test.atlas_engine.config_snapshot().await.collaboration_mode,
        original_collaboration_mode
    );
}

#[tokio::test]
async fn recover_turn_if_idle_preserves_id_and_resumes_plan_mode() {
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let test = test_atlas_engine()
        .build_with_auto_env(&server)
        .await
        .expect("build recovered turn session");
    let turn_id = "durable-recovered-turn";

    let submission = test
        .atlas_engine
        .recover_turn_if_idle(RecoverTurnRequest {
            turn_id: turn_id.to_string(),
            thread_settings: ThreadSettingsOverrides {
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Plan,
                    settings: Settings {
                        model: test.session_configured.model.clone(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
            trace: None,
        })
        .await
        .expect("recovered turn should start");
    assert_eq!(
        submission,
        StartIfIdleSubmission::Started {
            turn_id: turn_id.to_string(),
        }
    );

    let started = wait_for_event(&test.atlas_engine, |event| {
        matches!(event, EventMsg::TurnStarted(_))
    })
    .await;
    let EventMsg::TurnStarted(started) = started else {
        unreachable!("wait_for_event returned unexpected event");
    };
    assert_eq!(started.turn_id, turn_id);
    wait_for_event(&test.atlas_engine, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let user_input_groups = response_mock
        .single_request()
        .message_input_text_groups("user");
    assert_eq!(user_input_groups.len(), 1);
    assert_eq!(user_input_groups[0].len(), 1);
    assert!(user_input_groups[0][0].starts_with("<environment_context>"));
}

/// Concurrent submissions must start exactly one turn and steer the other message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_input_submission_reports_started_and_steered_for_concurrent_submissions() {
    let (release_response, response_gate) = oneshot::channel();
    let (server, _completions) = start_streaming_sse_server(vec![
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![ev_response_created("resp-1")]),
            },
            StreamingSseChunk {
                gate: Some(response_gate),
                body: responses::sse(vec![ev_completed("resp-1")]),
            },
        ],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
        }],
    ])
    .await;
    let test = test_atlas_engine()
        .with_model("gpt-5.4")
        .build_with_streaming_server(&server)
        .await
        .expect("build turn-input submission session");
    let atlas_engine = Arc::clone(&test.atlas_engine);
    let barrier = Arc::new(Barrier::new(3));

    let first_submission = tokio::spawn({
        let atlas_engine = Arc::clone(&atlas_engine);
        let barrier = Arc::clone(&barrier);
        async move {
            barrier.wait().await;
            submit_user_message(atlas_engine.as_ref(), "first message").await
        }
    });
    let second_submission = tokio::spawn({
        let atlas_engine = Arc::clone(&atlas_engine);
        let barrier = Arc::clone(&barrier);
        async move {
            barrier.wait().await;
            submit_user_message(atlas_engine.as_ref(), "second message").await
        }
    });
    barrier.wait().await;

    timeout(
        Duration::from_secs(5),
        server.wait_for_request_count(/*count*/ 1),
    )
    .await
    .expect("the started turn should reach its first model request");
    release_response
        .send(())
        .expect("response gate should remain open");

    let (first_submission, second_submission) = timeout(Duration::from_secs(5), async {
        tokio::join!(first_submission, second_submission)
    })
    .await
    .expect("both concurrent submissions should resolve once their messages are submitted");
    let first_submission = first_submission
        .expect("first submission task should finish")
        .expect("first user message should be submitted");
    let second_submission = second_submission
        .expect("second submission task should finish")
        .expect("second user message should be submitted");
    let (started_turn_id, steered_turn_id, started_message) =
        match (&first_submission, &second_submission) {
            (
                TurnInputSubmission::Started { turn_id: started },
                TurnInputSubmission::Steered { turn_id: steered },
            ) => (started, steered, "first message"),
            (
                TurnInputSubmission::Steered { turn_id: steered },
                TurnInputSubmission::Started { turn_id: started },
            ) => (started, steered, "second message"),
            _ => panic!(
                "concurrent messages must start exactly one turn and steer the other: \
             {first_submission:?}, {second_submission:?}"
            ),
        };
    assert_eq!(started_turn_id, steered_turn_id);

    wait_for_event(atlas_engine.as_ref(), |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = server.requests().await;
    assert_eq!(requests.len(), 2);
    let request_bodies: Vec<Value> = requests
        .iter()
        .map(|request| serde_json::from_slice(request).expect("parse model request"))
        .collect();
    assert!(request_bodies[0].to_string().contains(started_message));
    assert!(request_bodies[1].to_string().contains("first message"));
    assert!(request_bodies[1].to_string().contains("second message"));

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_input_submission_applies_thread_settings_only_after_accepted_input() {
    let (release_response, response_gate) = oneshot::channel();
    let (server, _completions) = start_streaming_sse_server(vec![
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![ev_response_created("resp-1")]),
            },
            StreamingSseChunk {
                gate: Some(response_gate),
                body: responses::sse(vec![ev_completed("resp-1")]),
            },
        ],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
        }],
    ])
    .await;
    let test = test_atlas_engine()
        .with_model("gpt-5.4")
        .with_config(|config| {
            config.permissions.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
        })
        .build_with_streaming_server(&server)
        .await
        .expect("build approval-constrained turn-input submission session");
    let atlas_engine = &test.atlas_engine;

    let started = submit_user_message(atlas_engine, "start turn")
        .await
        .expect("first message should start a turn");
    let TurnInputSubmission::Started { turn_id } = started else {
        panic!("first message should start a turn");
    };
    timeout(
        Duration::from_secs(5),
        server.wait_for_request_count(/*count*/ 1),
    )
    .await
    .expect("started turn should reach its first model request");

    let steered_cwd = test.config.cwd.join("steered-environment");
    let steered_environments =
        TurnEnvironmentSelections::new(steered_cwd.clone(), vec![local(steered_cwd)]);
    let steered = atlas_engine
        .start_or_steer_turn(
            user_message_request("steer active turn").with_thread_settings(
                ThreadSettingsOverrides {
                    approval_policy: Some(AskForApproval::Never),
                    environments: Some(steered_environments.clone()),
                    ..Default::default()
                },
            ),
        )
        .await
        .expect("persistent settings should not reject a steer");
    assert_eq!(steered, TurnInputSubmission::Steered { turn_id });
    assert_eq!(
        atlas_engine.config_snapshot().await.approval_policy,
        AskForApproval::Never
    );
    assert_eq!(
        atlas_engine.environment_selections().await,
        steered_environments.environments
    );

    release_response
        .send(())
        .expect("response gate should remain open");
    wait_for_event(atlas_engine, |event| matches!(event, EventMsg::TurnComplete(_))).await;

    let rejected_cwd = test.config.cwd.join("rejected-environment");
    let rejected = atlas_engine
        .steer_turn(
            user_message_request("no active turn").with_thread_settings(ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::OnRequest),
                environments: Some(TurnEnvironmentSelections::new(
                    rejected_cwd.clone(),
                    vec![local(rejected_cwd)],
                )),
                ..Default::default()
            }),
            "missing-turn".to_string(),
        )
        .await
        .expect("idle steer should return a typed rejection");
    assert_eq!(
        rejected,
        SteerSubmission::NotSubmitted {
            reason: NotSubmittedReason::NoActiveTurn,
        }
    );
    assert_eq!(
        atlas_engine.config_snapshot().await.approval_policy,
        AskForApproval::Never
    );
    assert_eq!(
        atlas_engine.environment_selections().await,
        steered_environments.environments
    );
    server.shutdown().await;
}

#[tokio::test]
async fn start_or_steer_turn_requires_matching_active_output_schema() {
    let (release_response, response_gate) = oneshot::channel();
    let (server, _completions) = start_streaming_sse_server(vec![
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![ev_response_created("resp-1")]),
            },
            StreamingSseChunk {
                gate: Some(response_gate),
                body: responses::sse(vec![ev_completed("resp-1")]),
            },
        ],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
        }],
    ])
    .await;
    let test = test_atlas_engine()
        .with_config(|config| {
            config.permissions.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
        })
        .build_with_streaming_server(&server)
        .await
        .expect("build turn-input submission session");
    let atlas_engine = &test.atlas_engine;
    let active_schema: Value = serde_json::from_str(
        r#"{"type":"object","properties":{"answer":{"type":"string"},"count":{"type":"number"}},"required":["answer","count"]}"#,
    )
    .expect("parse active schema");
    let matching_schema_with_different_object_order: Value = serde_json::from_str(
        r#"{"required":["answer","count"],"properties":{"count":{"type":"number"},"answer":{"type":"string"}},"type":"object"}"#,
    )
    .expect("parse matching schema");
    let different_schema: Value = serde_json::from_str(
        r#"{"type":"object","properties":{"answer":{"type":"number"}},"required":["answer"]}"#,
    )
    .expect("parse different schema");

    let started = atlas_engine
        .start_or_steer_turn(
            user_message_request("start turn").on_start(TurnStartOptions {
                final_output_json_schema: Some(active_schema),
                ..Default::default()
            }),
        )
        .await
        .expect("first message should start a turn");
    let TurnInputSubmission::Started { turn_id } = started else {
        panic!("first message should start a turn");
    };
    timeout(
        Duration::from_secs(5),
        server.wait_for_request_count(/*count*/ 1),
    )
    .await
    .expect("started turn should reach its first model request");

    let rejected = atlas_engine
        .start_or_steer_turn(
            user_message_request("rejected steer")
                .with_thread_settings(ThreadSettingsOverrides {
                    approval_policy: Some(AskForApproval::Never),
                    ..Default::default()
                })
                .on_start(TurnStartOptions {
                    final_output_json_schema: Some(different_schema),
                    ..Default::default()
                }),
        )
        .await
        .expect("schema mismatch should return a typed rejection");
    assert_eq!(
        rejected,
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::ActiveTurnOutputSchemaMismatch,
        }
    );
    assert_eq!(
        atlas_engine.config_snapshot().await.approval_policy,
        AskForApproval::OnRequest
    );

    let steered = atlas_engine
        .start_or_steer_turn(
            user_message_request("accepted steer").on_start(TurnStartOptions {
                final_output_json_schema: Some(matching_schema_with_different_object_order),
                ..Default::default()
            }),
        )
        .await
        .expect("matching schema should steer");
    assert_eq!(steered, TurnInputSubmission::Steered { turn_id });

    release_response
        .send(())
        .expect("response gate should remain open");
    wait_for_event(atlas_engine, |event| matches!(event, EventMsg::TurnComplete(_))).await;

    let requests = server.requests().await;
    assert_eq!(requests.len(), 2);
    let second_request = String::from_utf8_lossy(&requests[1]);
    assert!(second_request.contains("accepted steer"));
    assert!(!second_request.contains("rejected steer"));
    server.shutdown().await;
}
