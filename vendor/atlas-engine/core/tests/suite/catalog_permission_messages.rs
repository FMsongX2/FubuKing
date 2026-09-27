// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use anyhow::Result;
use atlas_engine_core::TurnInputRequest;
use atlas_engine_core::config::Constrained;
use atlas_engine_login::AtlasEngineAuth;
use atlas_engine_models_manager::manager::RefreshStrategy;
use atlas_engine_models_manager::model_info::model_info_from_slug;
use atlas_engine_protocol::models::PermissionProfile;
use atlas_engine_protocol::openai_models::ModelMessages;
use atlas_engine_protocol::openai_models::ModelsResponse;
use atlas_engine_protocol::openai_models::PermissionMessages;
use atlas_engine_protocol::protocol::AskForApproval;
use atlas_engine_protocol::protocol::EventMsg;
use atlas_engine_protocol::protocol::ThreadSettingsOverrides;
use atlas_engine_protocol::user_input::UserInput;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_models_once;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_atlas_engine::test_atlas_engine;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use wiremock::MockServer;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_permission_message_loaded_from_remote_models_is_sent() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let model_slug = "remote-catalog-permissions-model";
    let mut model = model_info_from_slug(model_slug);
    model.model_messages = Some(ModelMessages {
        instructions_template: None,
        instructions_variables: None,
        approvals: None,
        collaboration_modes: None,
        auto_review: None,
        permissions: Some(PermissionMessages {
            danger_full_access: None,
            workspace_write: None,
            read_only: Some("remote catalog permissions: {{ network_access }}".to_string()),
        }),
        multi_agent: None,
        token_budget: None,
    });
    let models_mock = mount_models_once(
        &server,
        ModelsResponse {
            models: vec![model],
        },
    )
    .await;
    let response_mock = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let mut builder = test_atlas_engine()
        .with_auth(AtlasEngineAuth::create_dummy_chatgpt_auth_for_testing())
        .with_config(|config| {
            config.permissions.approval_policy = Constrained::allow_any(AskForApproval::Never);
            config
                .permissions
                .set_permission_profile(PermissionProfile::read_only())
                .expect("read-only permission profile should be allowed");
        });
    let test = builder.build_with_auto_env(&server).await?;
    test.thread_manager
        .get_models_manager()
        .list_models(
            RefreshStrategy::OnlineIfUncached,
            atlas_engine_core::test_support::default_http_client_factory(),
        )
        .await;

    core_test_support::submit_thread_settings(
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
    wait_for_event(&test.atlas_engine, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    assert_eq!(models_mock.single_request_path(), "/v1/models");
    let permissions = response_mock
        .single_request()
        .message_input_texts("developer")
        .into_iter()
        .filter(|text| text.contains("<permissions instructions>"))
        .map(|text| text.replace("\r\n", "\n"))
        .collect::<Vec<_>>();
    assert_eq!(
        permissions,
        vec![
            "<permissions instructions>\nremote catalog permissions: restricted\nApproval policy is currently never. Do not provide the `sandbox_permissions` for any reason, commands will be rejected.\n</permissions instructions>"
                .to_string()
        ]
    );
    Ok(())
}
