// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use crate::bespoke_event_handling::apply_bespoke_event_handling;
use crate::command_exec::CommandExecManager;
use crate::command_exec::StartCommandExecParams;
use crate::config_manager::ConfigManager;
use crate::error_code::INPUT_TOO_LARGE_ERROR_CODE;
use crate::error_code::invalid_params;
use crate::models::supported_models;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::RequestContext;
use crate::outgoing_message::ThreadScopedOutgoingMessageSender;
use crate::skills_watcher::SkillsWatcher;
use crate::thread_status::ThreadWatchManager;
use crate::thread_status::resolve_thread_status;
use chrono::Duration as ChronoDuration;
use chrono::SecondsFormat;
use atlas_engine_analytics::AnalyticsEventsClient;
use atlas_engine_analytics::AnalyticsJsonRpcError;
use atlas_engine_analytics::InputError;
use atlas_engine_analytics::TurnSteerRequestError;
use atlas_engine_app_server_protocol::Account;
use atlas_engine_app_server_protocol::AccountLoginCompletedNotification;
use atlas_engine_app_server_protocol::AccountTokenUsageDailyBucket;
use atlas_engine_app_server_protocol::AccountTokenUsageSummary;
use atlas_engine_app_server_protocol::AccountUpdatedNotification;
use atlas_engine_app_server_protocol::AddCreditsNudgeCreditType;
use atlas_engine_app_server_protocol::AddCreditsNudgeEmailStatus;
use atlas_engine_app_server_protocol::AdditionalContextEntry;
use atlas_engine_app_server_protocol::AdditionalContextKind;
use atlas_engine_app_server_protocol::AppListUpdatedNotification;
use atlas_engine_app_server_protocol::AppSummary;
use atlas_engine_app_server_protocol::AppTemplateSummary;
use atlas_engine_app_server_protocol::AppTemplateUnavailableReason;
use atlas_engine_app_server_protocol::AppsInstalledParams;
use atlas_engine_app_server_protocol::AppsInstalledResponse;
use atlas_engine_app_server_protocol::AppsListParams;
use atlas_engine_app_server_protocol::AppsListResponse;
use atlas_engine_app_server_protocol::AppsReadParams;
use atlas_engine_app_server_protocol::AppsReadResponse;
use atlas_engine_app_server_protocol::AskForApproval;
use atlas_engine_app_server_protocol::AuthMode;
use atlas_engine_app_server_protocol::CancelLoginAccountParams;
use atlas_engine_app_server_protocol::CancelLoginAccountResponse;
use atlas_engine_app_server_protocol::CancelLoginAccountStatus;
use atlas_engine_app_server_protocol::ClientInfo;
use atlas_engine_app_server_protocol::ClientRequest;
use atlas_engine_app_server_protocol::ClientResponsePayload;
use atlas_engine_app_server_protocol::AtlasEngineErrorInfo;
use atlas_engine_app_server_protocol::CollaborationModeListParams;
use atlas_engine_app_server_protocol::CollaborationModeListResponse;
use atlas_engine_app_server_protocol::CommandExecParams;
use atlas_engine_app_server_protocol::CommandExecResizeParams;
use atlas_engine_app_server_protocol::CommandExecTerminateParams;
use atlas_engine_app_server_protocol::CommandExecWriteParams;
use atlas_engine_app_server_protocol::ConfigWarningNotification;
use atlas_engine_app_server_protocol::ConsumeAccountRateLimitResetCreditOutcome;
use atlas_engine_app_server_protocol::ConsumeAccountRateLimitResetCreditParams;
use atlas_engine_app_server_protocol::ConsumeAccountRateLimitResetCreditResponse;
use atlas_engine_app_server_protocol::ConversationGitInfo;
use atlas_engine_app_server_protocol::ConversationSummary;
use atlas_engine_app_server_protocol::DeprecationNoticeNotification;
use atlas_engine_app_server_protocol::DynamicToolFunctionSpec;
use atlas_engine_app_server_protocol::DynamicToolNamespaceTool;
use atlas_engine_app_server_protocol::DynamicToolSpec;
use atlas_engine_app_server_protocol::EnvironmentAddParams;
use atlas_engine_app_server_protocol::EnvironmentAddResponse;
use atlas_engine_app_server_protocol::EnvironmentInfoParams;
use atlas_engine_app_server_protocol::EnvironmentInfoResponse;
use atlas_engine_app_server_protocol::EnvironmentShellInfo;
use atlas_engine_app_server_protocol::EnvironmentStatusKind;
use atlas_engine_app_server_protocol::EnvironmentStatusParams;
use atlas_engine_app_server_protocol::EnvironmentStatusResponse;
use atlas_engine_app_server_protocol::ExperimentalFeature as ApiExperimentalFeature;
use atlas_engine_app_server_protocol::ExperimentalFeatureListParams;
use atlas_engine_app_server_protocol::ExperimentalFeatureListResponse;
use atlas_engine_app_server_protocol::ExperimentalFeatureStage as ApiExperimentalFeatureStage;
use atlas_engine_app_server_protocol::FeedbackUploadParams;
use atlas_engine_app_server_protocol::FeedbackUploadResponse;
use atlas_engine_app_server_protocol::GetAccountParams;
use atlas_engine_app_server_protocol::GetAccountRateLimitsResponse;
use atlas_engine_app_server_protocol::GetAccountResponse;
use atlas_engine_app_server_protocol::GetAccountTokenUsageParams;
use atlas_engine_app_server_protocol::GetAccountTokenUsageResponse;
use atlas_engine_app_server_protocol::GetAuthStatusParams;
use atlas_engine_app_server_protocol::GetAuthStatusResponse;
use atlas_engine_app_server_protocol::GetConversationSummaryParams;
use atlas_engine_app_server_protocol::GetConversationSummaryResponse;
use atlas_engine_app_server_protocol::GetWorkspaceMessagesResponse;
use atlas_engine_app_server_protocol::GitDiffToRemoteParams;
use atlas_engine_app_server_protocol::GitDiffToRemoteResponse;
use atlas_engine_app_server_protocol::GitInfo as ApiGitInfo;
use atlas_engine_app_server_protocol::HookMetadata;
use atlas_engine_app_server_protocol::HooksListParams;
use atlas_engine_app_server_protocol::HooksListResponse;
use atlas_engine_app_server_protocol::InitializeParams;
use atlas_engine_app_server_protocol::InitializeResponse;
use atlas_engine_app_server_protocol::InstalledApp;
use atlas_engine_app_server_protocol::JSONRPCErrorError;
use atlas_engine_app_server_protocol::ListMcpServerStatusParams;
use atlas_engine_app_server_protocol::ListMcpServerStatusResponse;
use atlas_engine_app_server_protocol::LoginAccountParams;
use atlas_engine_app_server_protocol::LoginAccountResponse;
use atlas_engine_app_server_protocol::LoginApiKeyParams;
use atlas_engine_app_server_protocol::LoginAppBrand;
use atlas_engine_app_server_protocol::LogoutAccountResponse;
use atlas_engine_app_server_protocol::MarketplaceAddParams;
use atlas_engine_app_server_protocol::MarketplaceAddResponse;
use atlas_engine_app_server_protocol::MarketplaceInterface;
use atlas_engine_app_server_protocol::MarketplaceRemoveParams;
use atlas_engine_app_server_protocol::MarketplaceRemoveResponse;
use atlas_engine_app_server_protocol::MarketplaceUpgradeErrorInfo;
use atlas_engine_app_server_protocol::MarketplaceUpgradeParams;
use atlas_engine_app_server_protocol::MarketplaceUpgradeResponse;
use atlas_engine_app_server_protocol::McpResourceReadParams;
use atlas_engine_app_server_protocol::McpResourceReadResponse;
use atlas_engine_app_server_protocol::McpServerOauthClientRegistration;
use atlas_engine_app_server_protocol::McpServerOauthLoginCompletedNotification;
use atlas_engine_app_server_protocol::McpServerOauthLoginParams;
use atlas_engine_app_server_protocol::McpServerOauthLoginResponse;
use atlas_engine_app_server_protocol::McpServerRefreshResponse;
use atlas_engine_app_server_protocol::McpServerStatus;
use atlas_engine_app_server_protocol::McpServerStatusDetail;
use atlas_engine_app_server_protocol::McpServerToolCallParams;
use atlas_engine_app_server_protocol::McpServerToolCallResponse;
use atlas_engine_app_server_protocol::MemoryResetResponse;
use atlas_engine_app_server_protocol::MockExperimentalMethodParams;
use atlas_engine_app_server_protocol::MockExperimentalMethodResponse;
use atlas_engine_app_server_protocol::ModelListParams;
use atlas_engine_app_server_protocol::ModelListResponse;
use atlas_engine_app_server_protocol::PermissionProfileListParams;
use atlas_engine_app_server_protocol::PermissionProfileListResponse;
use atlas_engine_app_server_protocol::PermissionProfileSummary;
use atlas_engine_app_server_protocol::PluginDetail;
use atlas_engine_app_server_protocol::PluginInstallParams;
use atlas_engine_app_server_protocol::PluginInstallResponse;
use atlas_engine_app_server_protocol::PluginInstalledParams;
use atlas_engine_app_server_protocol::PluginInstalledResponse;
use atlas_engine_app_server_protocol::PluginInterface;
use atlas_engine_app_server_protocol::PluginListMarketplaceKind;
use atlas_engine_app_server_protocol::PluginListParams;
use atlas_engine_app_server_protocol::PluginListResponse;
use atlas_engine_app_server_protocol::PluginMarketplaceEntry;
use atlas_engine_app_server_protocol::PluginReadParams;
use atlas_engine_app_server_protocol::PluginReadResponse;
use atlas_engine_app_server_protocol::PluginShareCheckoutParams;
use atlas_engine_app_server_protocol::PluginShareCheckoutResponse;
use atlas_engine_app_server_protocol::PluginShareContext;
use atlas_engine_app_server_protocol::PluginShareDeleteParams;
use atlas_engine_app_server_protocol::PluginShareDeleteResponse;
use atlas_engine_app_server_protocol::PluginShareDiscoverability;
use atlas_engine_app_server_protocol::PluginShareListItem;
use atlas_engine_app_server_protocol::PluginShareListParams;
use atlas_engine_app_server_protocol::PluginShareListResponse;
use atlas_engine_app_server_protocol::PluginSharePrincipal;
use atlas_engine_app_server_protocol::PluginSharePrincipalType;
use atlas_engine_app_server_protocol::PluginShareSaveParams;
use atlas_engine_app_server_protocol::PluginShareSaveResponse;
use atlas_engine_app_server_protocol::PluginShareTarget;
use atlas_engine_app_server_protocol::PluginShareUpdateDiscoverability;
use atlas_engine_app_server_protocol::PluginShareUpdateTargetsParams;
use atlas_engine_app_server_protocol::PluginShareUpdateTargetsResponse;
use atlas_engine_app_server_protocol::PluginSkillReadParams;
use atlas_engine_app_server_protocol::PluginSkillReadResponse;
use atlas_engine_app_server_protocol::PluginSource;
use atlas_engine_app_server_protocol::PluginSummary;
use atlas_engine_app_server_protocol::PluginUninstallParams;
use atlas_engine_app_server_protocol::PluginUninstallResponse;
use atlas_engine_app_server_protocol::RateLimitResetCredit;
use atlas_engine_app_server_protocol::RateLimitResetCreditStatus;
use atlas_engine_app_server_protocol::RateLimitResetCreditsSummary;
use atlas_engine_app_server_protocol::RateLimitResetType;
use atlas_engine_app_server_protocol::RequestId;
use atlas_engine_app_server_protocol::ReviewDelivery as ApiReviewDelivery;
use atlas_engine_app_server_protocol::ReviewStartParams;
use atlas_engine_app_server_protocol::ReviewStartResponse;
use atlas_engine_app_server_protocol::ReviewTarget as ApiReviewTarget;
use atlas_engine_app_server_protocol::SandboxMode;
use atlas_engine_app_server_protocol::SendAddCreditsNudgeEmailParams;
use atlas_engine_app_server_protocol::SendAddCreditsNudgeEmailResponse;
use atlas_engine_app_server_protocol::ServerNotification;
use atlas_engine_app_server_protocol::ServerRequestResolvedNotification;
use atlas_engine_app_server_protocol::SkillSummary;
use atlas_engine_app_server_protocol::SkillsConfigWriteParams;
use atlas_engine_app_server_protocol::SkillsConfigWriteResponse;
use atlas_engine_app_server_protocol::SkillsExtraRootsSetParams;
use atlas_engine_app_server_protocol::SkillsExtraRootsSetResponse;
use atlas_engine_app_server_protocol::SkillsListParams;
use atlas_engine_app_server_protocol::SkillsListResponse;
use atlas_engine_app_server_protocol::SortDirection;
use atlas_engine_app_server_protocol::Thread;
use atlas_engine_app_server_protocol::ThreadApproveGuardianDeniedActionParams;
use atlas_engine_app_server_protocol::ThreadApproveGuardianDeniedActionResponse;
use atlas_engine_app_server_protocol::ThreadArchiveParams;
use atlas_engine_app_server_protocol::ThreadArchiveResponse;
use atlas_engine_app_server_protocol::ThreadArchivedNotification;
use atlas_engine_app_server_protocol::ThreadBackgroundTerminal;
use atlas_engine_app_server_protocol::ThreadBackgroundTerminalsCleanParams;
use atlas_engine_app_server_protocol::ThreadBackgroundTerminalsCleanResponse;
use atlas_engine_app_server_protocol::ThreadBackgroundTerminalsListParams;
use atlas_engine_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use atlas_engine_app_server_protocol::ThreadBackgroundTerminalsTerminateParams;
use atlas_engine_app_server_protocol::ThreadBackgroundTerminalsTerminateResponse;
use atlas_engine_app_server_protocol::ThreadClosedNotification;
use atlas_engine_app_server_protocol::ThreadCompactStartParams;
use atlas_engine_app_server_protocol::ThreadCompactStartResponse;
use atlas_engine_app_server_protocol::ThreadDecrementElicitationParams;
use atlas_engine_app_server_protocol::ThreadDecrementElicitationResponse;
use atlas_engine_app_server_protocol::ThreadDeleteParams;
use atlas_engine_app_server_protocol::ThreadDeleteResponse;
use atlas_engine_app_server_protocol::ThreadDeletedNotification;
use atlas_engine_app_server_protocol::ThreadForkParams;
use atlas_engine_app_server_protocol::ThreadForkResponse;
use atlas_engine_app_server_protocol::ThreadGoal;
use atlas_engine_app_server_protocol::ThreadGoalClearParams;
use atlas_engine_app_server_protocol::ThreadGoalClearResponse;
use atlas_engine_app_server_protocol::ThreadGoalClearedNotification;
use atlas_engine_app_server_protocol::ThreadGoalGetParams;
use atlas_engine_app_server_protocol::ThreadGoalGetResponse;
use atlas_engine_app_server_protocol::ThreadGoalSetParams;
use atlas_engine_app_server_protocol::ThreadGoalSetResponse;
use atlas_engine_app_server_protocol::ThreadGoalStatus;
use atlas_engine_app_server_protocol::ThreadGoalUpdatedNotification;
use atlas_engine_app_server_protocol::ThreadHistoryBuilder;
#[cfg(test)]
use atlas_engine_app_server_protocol::ThreadHistoryMode;
use atlas_engine_app_server_protocol::ThreadIncrementElicitationParams;
use atlas_engine_app_server_protocol::ThreadIncrementElicitationResponse;
use atlas_engine_app_server_protocol::ThreadInjectItemsParams;
use atlas_engine_app_server_protocol::ThreadInjectItemsResponse;
use atlas_engine_app_server_protocol::ThreadItem;
use atlas_engine_app_server_protocol::ThreadItemEntry;
use atlas_engine_app_server_protocol::ThreadItemsListParams;
use atlas_engine_app_server_protocol::ThreadItemsListResponse;
use atlas_engine_app_server_protocol::ThreadListCwdFilter;
use atlas_engine_app_server_protocol::ThreadListParams;
use atlas_engine_app_server_protocol::ThreadListResponse;
use atlas_engine_app_server_protocol::ThreadLoadedListParams;
use atlas_engine_app_server_protocol::ThreadLoadedListResponse;
use atlas_engine_app_server_protocol::ThreadMemoryModeSetParams;
use atlas_engine_app_server_protocol::ThreadMemoryModeSetResponse;
use atlas_engine_app_server_protocol::ThreadMetadataGitInfoUpdateParams;
use atlas_engine_app_server_protocol::ThreadMetadataUpdateParams;
use atlas_engine_app_server_protocol::ThreadMetadataUpdateResponse;
use atlas_engine_app_server_protocol::ThreadNameUpdatedNotification;
use atlas_engine_app_server_protocol::ThreadReadParams;
use atlas_engine_app_server_protocol::ThreadReadResponse;
use atlas_engine_app_server_protocol::ThreadRealtimeAppendAudioParams;
use atlas_engine_app_server_protocol::ThreadRealtimeAppendAudioResponse;
use atlas_engine_app_server_protocol::ThreadRealtimeAppendSpeechParams;
use atlas_engine_app_server_protocol::ThreadRealtimeAppendSpeechResponse;
use atlas_engine_app_server_protocol::ThreadRealtimeAppendTextParams;
use atlas_engine_app_server_protocol::ThreadRealtimeAppendTextResponse;
use atlas_engine_app_server_protocol::ThreadRealtimeListVoicesResponse;
use atlas_engine_app_server_protocol::ThreadRealtimeStartParams;
use atlas_engine_app_server_protocol::ThreadRealtimeStartResponse;
use atlas_engine_app_server_protocol::ThreadRealtimeStartTransport;
use atlas_engine_app_server_protocol::ThreadRealtimeStopParams;
use atlas_engine_app_server_protocol::ThreadRealtimeStopResponse;
use atlas_engine_app_server_protocol::ThreadResumeInitialTurnsPageParams;
use atlas_engine_app_server_protocol::ThreadResumeParams;
use atlas_engine_app_server_protocol::ThreadResumeResponse;
use atlas_engine_app_server_protocol::ThreadRollbackParams;
use atlas_engine_app_server_protocol::ThreadSearchOccurrence;
use atlas_engine_app_server_protocol::ThreadSearchOccurrencesParams;
use atlas_engine_app_server_protocol::ThreadSearchOccurrencesResponse;
use atlas_engine_app_server_protocol::ThreadSearchParams;
use atlas_engine_app_server_protocol::ThreadSearchResponse;
use atlas_engine_app_server_protocol::ThreadSearchResult;
use atlas_engine_app_server_protocol::ThreadSearchSortKey;
use atlas_engine_app_server_protocol::ThreadSearchTextRange;
use atlas_engine_app_server_protocol::ThreadSetNameParams;
use atlas_engine_app_server_protocol::ThreadSetNameResponse;
use atlas_engine_app_server_protocol::ThreadSettings;
use atlas_engine_app_server_protocol::ThreadSettingsUpdateParams;
use atlas_engine_app_server_protocol::ThreadSettingsUpdateResponse;
use atlas_engine_app_server_protocol::ThreadShellCommandParams;
use atlas_engine_app_server_protocol::ThreadShellCommandResponse;
use atlas_engine_app_server_protocol::ThreadSortKey;
use atlas_engine_app_server_protocol::ThreadSourceKind;
use atlas_engine_app_server_protocol::ThreadStartParams;
use atlas_engine_app_server_protocol::ThreadStartResponse;
use atlas_engine_app_server_protocol::ThreadStartedNotification;
use atlas_engine_app_server_protocol::ThreadStatus;
use atlas_engine_app_server_protocol::ThreadTurnsListParams;
use atlas_engine_app_server_protocol::ThreadTurnsListResponse;
use atlas_engine_app_server_protocol::ThreadUnarchiveParams;
use atlas_engine_app_server_protocol::ThreadUnarchiveResponse;
use atlas_engine_app_server_protocol::ThreadUnarchivedNotification;
use atlas_engine_app_server_protocol::ThreadUnsubscribeParams;
use atlas_engine_app_server_protocol::ThreadUnsubscribeResponse;
use atlas_engine_app_server_protocol::ThreadUnsubscribeStatus;
use atlas_engine_app_server_protocol::Turn;
use atlas_engine_app_server_protocol::TurnEnvironmentParams;
use atlas_engine_app_server_protocol::TurnError;
use atlas_engine_app_server_protocol::TurnInterruptParams;
use atlas_engine_app_server_protocol::TurnInterruptResponse;
use atlas_engine_app_server_protocol::TurnItemsView;
use atlas_engine_app_server_protocol::TurnStartParams;
use atlas_engine_app_server_protocol::TurnStartResponse;
use atlas_engine_app_server_protocol::TurnStatus;
use atlas_engine_app_server_protocol::TurnSteerParams;
use atlas_engine_app_server_protocol::TurnSteerResponse;
use atlas_engine_app_server_protocol::UserInput as V2UserInput;
use atlas_engine_app_server_protocol::WindowsSandboxReadiness;
use atlas_engine_app_server_protocol::WindowsSandboxReadinessResponse;
use atlas_engine_app_server_protocol::WindowsSandboxSetupCompletedNotification;
use atlas_engine_app_server_protocol::WindowsSandboxSetupMode;
use atlas_engine_app_server_protocol::WindowsSandboxSetupStartParams;
use atlas_engine_app_server_protocol::WindowsSandboxSetupStartResponse;
use atlas_engine_app_server_protocol::WorkspaceMessage;
use atlas_engine_app_server_protocol::WorkspaceMessageType;
use atlas_engine_arg0::Arg0DispatchPaths;
use atlas_engine_backend_client::AddCreditsNudgeCreditType as BackendAddCreditsNudgeCreditType;
use atlas_engine_backend_client::Client as BackendClient;
use atlas_engine_backend_client::AtlasEngineWorkspaceMessage as BackendWorkspaceMessage;
use atlas_engine_backend_client::AtlasEngineWorkspaceMessageType as BackendWorkspaceMessageType;
use atlas_engine_backend_client::AtlasEngineWorkspaceMessagesResponse as BackendWorkspaceMessagesResponse;
use atlas_engine_backend_client::ConsumeRateLimitResetCreditCode as BackendConsumeRateLimitResetCreditCode;
use atlas_engine_backend_client::RateLimitResetCreditDetails as BackendRateLimitResetCreditDetails;
use atlas_engine_backend_client::RateLimitResetCreditsDetails as BackendRateLimitResetCreditsDetails;
use atlas_engine_backend_client::RequestError as BackendRequestError;
use atlas_engine_backend_client::TokenUsageProfile;
use atlas_engine_chatgpt::connectors;
use atlas_engine_chatgpt::workspace_settings;
use atlas_engine_config::CloudConfigBundleLoadError;
use atlas_engine_config::CloudConfigBundleLoadErrorCode;
use atlas_engine_config::ConfigLayerStack;
use atlas_engine_config::loader::project_trust_key;
use atlas_engine_config::types::McpServerTransportConfig;
use atlas_engine_connectors::AppInfo;
use atlas_engine_core::AtlasEngineThread;
use atlas_engine_core::AtlasEngineThreadSettingsOverrides;
use atlas_engine_core::ForkSnapshot;
use atlas_engine_core::McpManager;
use atlas_engine_core::NewThread;
use atlas_engine_core::NotSubmittedReason;
#[cfg(test)]
use atlas_engine_core::SessionMeta;
use atlas_engine_core::StartThreadOptions;
use atlas_engine_core::SteerSubmission;
use atlas_engine_core::ThreadConfigSnapshot;
use atlas_engine_core::ThreadManager;
use atlas_engine_core::TurnInput;
use atlas_engine_core::TurnInputRequest;
use atlas_engine_core::TurnInputSubmission;
use atlas_engine_core::TurnStartOptions;
use atlas_engine_core::config::Config;
use atlas_engine_core::config::ConfigOverrides;
use atlas_engine_core::config::NetworkProxyAuditMetadata;
use atlas_engine_core::config::edit::ConfigEdit;
use atlas_engine_core::config::edit::ConfigEditsBuilder;
use atlas_engine_core::connectors::AccessibleConnectorsStatus;
use atlas_engine_core::exec::ExecCapturePolicy;
use atlas_engine_core::exec::ExecExpiration;
use atlas_engine_core::exec::ExecParams;
use atlas_engine_core::exec_env::create_env;
use atlas_engine_core::path_utils;
#[cfg(test)]
use atlas_engine_core::read_head_for_summary;
use atlas_engine_core::sandboxing::SandboxPermissions;
use atlas_engine_core::truncate_rollout_after_turn_id;
use atlas_engine_core::truncate_rollout_before_turn_id;
use atlas_engine_core::windows_sandbox::WindowsSandboxLevelExt;
use atlas_engine_core::windows_sandbox::WindowsSandboxSetupMode as CoreWindowsSandboxSetupMode;
use atlas_engine_core::windows_sandbox::WindowsSandboxSetupRequest;
use atlas_engine_core::windows_sandbox::sandbox_setup_is_complete;
use atlas_engine_core_plugins::PluginInstallError as CorePluginInstallError;
use atlas_engine_core_plugins::PluginInstallRequest;
use atlas_engine_core_plugins::PluginReadRequest;
use atlas_engine_core_plugins::PluginUninstallError as CorePluginUninstallError;
use atlas_engine_core_plugins::PluginsManager;
use atlas_engine_core_plugins::loader::load_plugin_apps;
use atlas_engine_core_plugins::manifest::PluginManifestInterface;
use atlas_engine_core_plugins::marketplace::MarketplaceError;
use atlas_engine_core_plugins::marketplace::MarketplacePluginSource;
use atlas_engine_core_plugins::marketplace_add::MarketplaceAddError;
use atlas_engine_core_plugins::marketplace_add::MarketplaceAddRequest;
use atlas_engine_core_plugins::marketplace_add::add_marketplace as add_marketplace_to_atlas_agent_home;
use atlas_engine_core_plugins::marketplace_remove::MarketplaceRemoveError;
use atlas_engine_core_plugins::marketplace_remove::MarketplaceRemoveRequest as CoreMarketplaceRemoveRequest;
use atlas_engine_core_plugins::marketplace_remove::remove_marketplace;
use atlas_engine_core_plugins::remote::RemoteMarketplace;
use atlas_engine_core_plugins::remote::RemoteMarketplaceSource;
use atlas_engine_core_plugins::remote::RemotePluginCatalogError;
use atlas_engine_core_plugins::remote::RemotePluginDetail as RemoteCatalogPluginDetail;
use atlas_engine_core_plugins::remote::RemotePluginServiceConfig;
use atlas_engine_core_plugins::remote::RemotePluginShareContext as RemoteCatalogPluginShareContext;
use atlas_engine_core_plugins::remote::RemotePluginShareSummary as RemoteCatalogPluginShareSummary;
use atlas_engine_core_plugins::remote::RemotePluginSummary as RemoteCatalogPluginSummary;
use atlas_engine_exec_server::EnvironmentManager;
use atlas_engine_exec_server::EnvironmentObservedStatus;
use atlas_engine_exec_server::LOCAL_ENVIRONMENT_ID;
use atlas_engine_exec_server::LOCAL_FS;
use atlas_engine_features::FEATURES;
use atlas_engine_features::Feature;
use atlas_engine_features::Stage;
use atlas_engine_feedback::AtlasEngineFeedback;
use atlas_engine_feedback::FeedbackAttachmentPath;
use atlas_engine_feedback::FeedbackUploadOptions;
use atlas_engine_git_utils::git_diff_to_remote;
use atlas_engine_git_utils::resolve_root_git_project_for_trust;
use atlas_engine_login::AuthManager;
use atlas_engine_login::ATLAS_AGENT_OPEN_APP_URL;
use atlas_engine_login::AtlasEngineAuth;
use atlas_engine_login::LoginSuccessPage;
use atlas_engine_login::LoginSuccessPageBrand;
use atlas_engine_login::ServerOptions as LoginServerOptions;
use atlas_engine_login::ShutdownHandle;
use atlas_engine_login::complete_device_code_login;
use atlas_engine_login::login_with_api_key;
use atlas_engine_login::login_with_bedrock_api_key;
use atlas_engine_login::oauth_client_id;
use atlas_engine_login::request_device_code;
use atlas_engine_login::run_login_server;
use atlas_engine_mcp::McpRuntimeContext;
use atlas_engine_mcp::McpServerStatusSnapshot;
use atlas_engine_mcp::McpSnapshotDetail;
use atlas_engine_mcp::collect_mcp_server_status_snapshot_with_detail;
use atlas_engine_mcp::discover_supported_scopes;
use atlas_engine_mcp::read_mcp_resource as read_mcp_resource_without_thread;
use atlas_engine_mcp::resolve_oauth_scopes;
use atlas_engine_memories_write::clear_memory_roots_contents;
use atlas_engine_model_provider::create_model_provider;
use atlas_engine_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets;
use atlas_engine_protocol::ThreadId;
use atlas_engine_protocol::config_types::CollaborationMode;
use atlas_engine_protocol::config_types::ForcedLoginMethod;
use atlas_engine_protocol::config_types::Personality;
use atlas_engine_protocol::config_types::ReasoningSummary;
use atlas_engine_protocol::config_types::TrustLevel;
use atlas_engine_protocol::config_types::WindowsSandboxLevel;
use atlas_engine_protocol::error::AtlasEngineErr;
use atlas_engine_protocol::error::Result as AtlasEngineResult;
#[cfg(test)]
use atlas_engine_protocol::items::TurnItem;
use atlas_engine_protocol::models::ResponseItem;
use atlas_engine_protocol::openai_models::ReasoningEffort;
use atlas_engine_protocol::protocol::AgentStatus;
use atlas_engine_protocol::protocol::ConversationAudioParams;
use atlas_engine_protocol::protocol::ConversationSpeechParams;
use atlas_engine_protocol::protocol::ConversationStartParams;
use atlas_engine_protocol::protocol::ConversationStartTransport;
use atlas_engine_protocol::protocol::ConversationTextParams;
use atlas_engine_protocol::protocol::EnvironmentConfigState;
use atlas_engine_protocol::protocol::EventMsg;
#[cfg(test)]
use atlas_engine_protocol::protocol::GitInfo as CoreGitInfo;
use atlas_engine_protocol::protocol::McpAuthStatus as CoreMcpAuthStatus;
use atlas_engine_protocol::protocol::Op;
use atlas_engine_protocol::protocol::RealtimeVoicesList;
use atlas_engine_protocol::protocol::ReviewDelivery as CoreReviewDelivery;
use atlas_engine_protocol::protocol::ReviewRequest;
use atlas_engine_protocol::protocol::ReviewTarget as CoreReviewTarget;
use atlas_engine_protocol::protocol::SessionConfiguredEvent;
#[cfg(test)]
use atlas_engine_protocol::protocol::SessionMetaLine;
use atlas_engine_protocol::protocol::TurnEnvironmentSelection;
use atlas_engine_protocol::protocol::TurnEnvironmentSelections;
use atlas_engine_protocol::protocol::W3cTraceContext;
use atlas_engine_protocol::protocol::strip_user_message_prefix;
use atlas_engine_protocol::user_input::MAX_USER_INPUT_TEXT_CHARS;
use atlas_engine_protocol::user_input::UserInput as CoreInputItem;
use atlas_engine_rmcp_client::McpOAuthClientRegistration;
use atlas_engine_rmcp_client::StreamableHttpRedirectMode;
use atlas_engine_rmcp_client::perform_oauth_login_return_url;
use atlas_engine_rollout::InitialHistory;
use atlas_engine_rollout::ResumedHistory;
use atlas_engine_rollout::RolloutItem;
use atlas_engine_rollout::is_persisted_rollout_item;
use atlas_engine_rollout::state_db::StateDbHandle;
use atlas_engine_rollout::state_db::reconcile_rollout;
use atlas_engine_state::ThreadMetadata;
use atlas_engine_state::log_db::LogDbLayer;
use atlas_engine_thread_store::ArchiveThreadParams as StoreArchiveThreadParams;
use atlas_engine_thread_store::ArchiveThreadsParams as StoreArchiveThreadsParams;
use atlas_engine_thread_store::DeleteThreadsParams as StoreDeleteThreadsParams;
use atlas_engine_thread_store::GitInfoPatch as StoreGitInfoPatch;
use atlas_engine_thread_store::ItemSortKey as StoreItemSortKey;
use atlas_engine_thread_store::ListItemsParams as StoreListItemsParams;
use atlas_engine_thread_store::ListThreadsParams as StoreListThreadsParams;
use atlas_engine_thread_store::ListTurnsParams as StoreListTurnsParams;
use atlas_engine_thread_store::LoadThreadHistoryParams as StoreLoadThreadHistoryParams;
use atlas_engine_thread_store::LocalThreadStore;
use atlas_engine_thread_store::ReadThreadByRolloutPathParams as StoreReadThreadByRolloutPathParams;
use atlas_engine_thread_store::ReadThreadParams as StoreReadThreadParams;
use atlas_engine_thread_store::SearchThreadOccurrencesParams as StoreSearchThreadOccurrencesParams;
use atlas_engine_thread_store::SearchThreadsParams as StoreSearchThreadsParams;
use atlas_engine_thread_store::SortDirection as StoreSortDirection;
use atlas_engine_thread_store::StoredThread;
use atlas_engine_thread_store::StoredTurn;
use atlas_engine_thread_store::StoredTurnItemsView;
use atlas_engine_thread_store::StoredTurnStatus;
use atlas_engine_thread_store::ThreadMetadataPatch as StoreThreadMetadataPatch;
use atlas_engine_thread_store::ThreadRelationFilter as StoreThreadRelationFilter;
use atlas_engine_thread_store::ThreadSortKey as StoreThreadSortKey;
use atlas_engine_thread_store::ThreadStore;
use atlas_engine_thread_store::ThreadStoreError;
use atlas_engine_utils_absolute_path::AbsolutePathBuf;
use atlas_engine_utils_pty::DEFAULT_OUTPUT_BYTES_CAP;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;
use tokio_util::task::TaskTracker;
use toml::Value as TomlValue;
use tracing::Instrument;
use tracing::error;
use tracing::info;
use tracing::warn;
use uuid::Uuid;

#[cfg(test)]
use atlas_engine_app_server_protocol::ServerRequest;

mod account_processor;
mod apps_processor;
mod bedrock_auth;
mod catalog_processor;
mod command_exec_processor;
mod config_processor;
mod diagnostics;
mod environment_processor;
mod feedback_doctor_report;
mod feedback_processor;
mod fs_processor;
mod git_processor;
mod initialize_processor;
mod marketplace_processor;
mod mcp_processor;
mod plugins;
mod process_exec_processor;
mod remote_control_processor;
mod search;
mod thread_enrichment;
mod thread_fork_goal;
mod thread_processor;
mod thread_queue_processor;
mod thread_sections;
mod token_usage_replay;
mod turn_processor;
mod windows_sandbox_processor;

pub(crate) use account_processor::AccountRequestProcessor;
pub(crate) use apps_processor::AppsRequestProcessor;
pub(crate) use catalog_processor::CatalogRequestProcessor;
pub(crate) use command_exec_processor::CommandExecRequestProcessor;
pub(crate) use config_processor::ConfigRequestProcessor;
pub(crate) use diagnostics::read_server_diagnostics;
pub(crate) use environment_processor::EnvironmentRequestProcessor;
pub(crate) use feedback_processor::FeedbackRequestProcessor;
pub(crate) use fs_processor::FsRequestProcessor;
pub(crate) use git_processor::GitRequestProcessor;
pub(crate) use initialize_processor::InitializeRequestProcessor;
pub(crate) use marketplace_processor::MarketplaceRequestProcessor;
pub(crate) use mcp_processor::McpRequestProcessor;
pub(crate) use plugins::PluginRequestProcessor;
pub(crate) use process_exec_processor::ProcessExecRequestProcessor;
pub(crate) use remote_control_processor::RemoteControlRequestProcessor;
pub(crate) use search::SearchRequestProcessor;
pub(crate) use thread_goal_processor::ThreadGoalRequestProcessor;
pub(crate) use thread_processor::ThreadRequestProcessor;
pub(crate) use thread_queue_processor::ThreadQueueRequestProcessor;
pub(crate) use turn_processor::TurnRequestProcessor;
pub(crate) use windows_sandbox_processor::WindowsSandboxRequestProcessor;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::filters::compute_source_filters;
use crate::filters::source_kind_matches;
use crate::thread_state::ConnectionCapabilities;
use crate::thread_state::ThreadListenerCommand;
use crate::thread_state::ThreadState;
use crate::thread_state::ThreadStateManager;
use token_usage_replay::restored_token_usage_turn_id;
use token_usage_replay::send_thread_token_usage_update_to_connection;

fn resolve_request_cwd(cwd: Option<PathBuf>) -> Result<Option<AbsolutePathBuf>, JSONRPCErrorError> {
    cwd.map(|cwd| {
        AbsolutePathBuf::relative_to_current_dir(path_utils::normalize_for_native_workdir(cwd))
            .map_err(|err| invalid_request(format!("invalid cwd: {err}")))
    })
    .transpose()
}

fn resolve_turn_environment_selections(
    thread_manager: &ThreadManager,
    environments: Option<Vec<TurnEnvironmentParams>>,
) -> Result<Option<Vec<TurnEnvironmentSelection>>, JSONRPCErrorError> {
    let Some(environments) = environments else {
        return Ok(None);
    };
    let mut selections = Vec::with_capacity(environments.len());
    for environment in environments {
        let environment_id = environment.environment_id;
        let cwd = environment
            .cwd
            .to_inferred_path_uri()
            .ok_or_else(|| {
                invalid_request(format!(
                    "invalid cwd for environment `{environment_id}`: path `{}` does not use absolute POSIX or Windows path syntax",
                    environment.cwd
                ))
            })?;
        let workspace_roots = environment
            .runtime_workspace_roots
            .map(|roots| {
                let mut resolved_roots = Vec::new();
                for root in roots {
                    let root = root.to_inferred_path_uri().ok_or_else(|| {
                        invalid_request(format!(
                            "invalid runtime workspace root for environment `{environment_id}`: path `{root}` does not use absolute POSIX or Windows path syntax"
                        ))
                    })?;
                    if !resolved_roots.contains(&root) {
                        resolved_roots.push(root);
                    }
                }
                Ok::<_, JSONRPCErrorError>(resolved_roots)
            })
            .transpose()?
            .unwrap_or_else(|| vec![cwd.clone()]);
        selections.push(TurnEnvironmentSelection {
            environment_id,
            cwd,
            workspace_roots,
            config: EnvironmentConfigState::FromThread,
        });
    }
    thread_manager
        .validate_environment_selections(&selections)
        .map_err(environment_selection_error)?;
    Ok(Some(selections))
}

fn resolve_runtime_workspace_roots(workspace_roots: Vec<AbsolutePathBuf>) -> Vec<AbsolutePathBuf> {
    let mut resolved_roots = Vec::new();
    for root in workspace_roots {
        if !resolved_roots.iter().any(|existing| existing == &root) {
            resolved_roots.push(root);
        }
    }
    resolved_roots
}

mod config_errors;
mod request_errors;
mod thread_delete;
mod thread_goal_processor;
mod thread_lifecycle;
mod thread_resume_redaction;
mod thread_summary;

use self::config_errors::*;
use self::request_errors::*;
use self::thread_goal_processor::api_thread_goal_from_state;
use self::thread_lifecycle::*;
use self::thread_resume_redaction::*;
use self::thread_summary::*;

pub(crate) use self::thread_lifecycle::populate_thread_turns_from_history;
pub(crate) use self::thread_processor::thread_from_stored_thread;
#[cfg(test)]
pub(crate) use self::thread_summary::read_summary_from_rollout;
#[cfg(test)]
pub(crate) use self::thread_summary::summary_to_thread;
pub(crate) use self::thread_summary::thread_settings_from_config_snapshot;
pub(crate) use self::thread_summary::thread_settings_from_core_snapshot;

pub(crate) fn build_legacy_api_turns_from_rollout_items(items: &[RolloutItem]) -> Vec<Turn> {
    let mut builder = ThreadHistoryBuilder::new();
    for item in items {
        if is_persisted_rollout_item(item, atlas_engine_protocol::protocol::ThreadHistoryMode::Legacy) {
            builder.handle_rollout_item(item);
        }
    }
    builder.finish()
}
