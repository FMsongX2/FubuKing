#![cfg(not(target_os = "windows"))]
// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.

use atlas_engine_core::TurnInputRequest;
use atlas_engine_features::Feature;
use atlas_engine_login::AtlasEngineAuth;
use atlas_engine_protocol::protocol::EventMsg;
use atlas_engine_protocol::user_input::UserInput;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_atlas_engine::test_atlas_engine;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_body_is_zstd_compressed_for_atlas_engine_backend_when_enabled() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let request_log = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let base_url = format!("{}/backend-api/atlas-agent/v1", server.uri());
    let mut builder = test_atlas_engine()
        .with_auth(AtlasEngineAuth::create_dummy_chatgpt_auth_for_testing())
        .with_config(move |config| {
            config
                .features
                .enable(Feature::EnableRequestCompression)
                .expect("test config should allow feature update");
            config.model_provider.base_url = Some(base_url);
        });
    let atlas_engine = builder.build(&server).await?.atlas_engine;

    atlas_engine
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "compress me".into(),
            text_elements: Vec::new(),
        }]))
        .await?;

    // Wait until the task completes so the request definitely hit the server.
    wait_for_event(&atlas_engine, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request = request_log.single_request();
    assert_eq!(request.header("content-encoding").as_deref(), Some("zstd"));

    let decompressed = zstd::stream::decode_all(std::io::Cursor::new(request.body_bytes()))?;
    let json: serde_json::Value = serde_json::from_slice(&decompressed)?;
    assert!(
        json.get("input").is_some(),
        "expected request body to decode as Responses API JSON"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_body_is_not_compressed_for_api_key_auth_even_when_enabled() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let request_log = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let base_url = format!("{}/backend-api/atlas-agent/v1", server.uri());
    let mut builder = test_atlas_engine().with_config(move |config| {
        config
            .features
            .enable(Feature::EnableRequestCompression)
            .expect("test config should allow feature update");
        config.model_provider.base_url = Some(base_url);
    });
    let atlas_engine = builder.build(&server).await?.atlas_engine;

    atlas_engine
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "do not compress".into(),
            text_elements: Vec::new(),
        }]))
        .await?;

    wait_for_event(&atlas_engine, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request = request_log.single_request();
    assert!(
        request.header("content-encoding").is_none(),
        "did not expect request compression for API-key auth"
    );

    let json: serde_json::Value = serde_json::from_slice(&request.body_bytes())?;
    assert!(
        json.get("input").is_some(),
        "expected request body to be plain Responses API JSON"
    );

    Ok(())
}
