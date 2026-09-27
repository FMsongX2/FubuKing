// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use anyhow::Result;
use atlas_engine_core::TurnInputRequest;
use atlas_engine_protocol::protocol::EventMsg;
use atlas_engine_protocol::user_input::UserInput;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_atlas_engine::test_atlas_engine;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quota_exceeded_emits_single_error_event() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let mut builder = test_atlas_engine();

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            json!({
                "type": "response.failed",
                "response": {
                    "id": "resp-1",
                    "error": {
                        "code": "insufficient_quota",
                        "message": "You exceeded your current quota, please check your plan and billing details."
                    }
                }
            }),
        ]),
    )
    .await;

    let test = builder.build(&server).await?;

    test.atlas_engine
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "quota?".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .unwrap();

    let mut error_events = 0;

    loop {
        let event = wait_for_event(&test.atlas_engine, |_| true).await;

        match event {
            EventMsg::Error(err) => {
                error_events += 1;
                assert_eq!(
                    err.message,
                    "Quota exceeded. Check your plan and billing details."
                );
            }
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }

    assert_eq!(error_events, 1, "expected exactly one Atlas Agent:Error event");

    Ok(())
}
