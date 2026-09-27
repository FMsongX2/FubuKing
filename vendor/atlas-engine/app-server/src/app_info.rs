// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_app_server_protocol::AppBranding as ApiAppBranding;
use atlas_engine_app_server_protocol::AppInfo as ApiAppInfo;
use atlas_engine_app_server_protocol::AppMetadata as ApiAppMetadata;
use atlas_engine_app_server_protocol::AppReview as ApiAppReview;
use atlas_engine_app_server_protocol::AppScreenshot as ApiAppScreenshot;
use atlas_engine_app_server_protocol::AppToolSummary as ApiAppToolSummary;
use atlas_engine_app_server_protocol::ConnectorMetadata as ApiConnectorMetadata;
use atlas_engine_connectors::AppBranding;
use atlas_engine_connectors::AppInfo;
use atlas_engine_connectors::AppMetadata;
use atlas_engine_connectors::AppReview;
use atlas_engine_connectors::AppScreenshot;
use atlas_engine_connectors::ConnectorMetadata;
use atlas_engine_connectors::ConnectorToolSummary;
use atlas_engine_connectors::metadata::connector_install_url;

/// Converts connector-domain app metadata owned by `atlas-engine-connectors` into the app-server wire
/// type owned by `atlas-engine-app-server-protocol`.
///
/// The types stay separate so app-server protocol ownership does not leak into the connector
/// domain crate. Because this crate owns neither type, Rust's orphan rules require an explicit
/// conversion function instead of a `From` implementation.
pub(crate) fn app_info_to_api(app: AppInfo) -> ApiAppInfo {
    let AppInfo {
        id,
        name,
        description,
        logo_url,
        logo_url_dark,
        icon_assets,
        icon_dark_assets,
        distribution_channel,
        branding,
        app_metadata,
        labels,
        install_url,
        is_accessible,
        is_enabled,
        plugin_display_names,
    } = app;
    ApiAppInfo {
        id,
        name,
        description,
        logo_url,
        logo_url_dark,
        icon_assets,
        icon_dark_assets,
        distribution_channel,
        branding: branding.map(app_branding_to_api),
        app_metadata: app_metadata.map(app_metadata_to_api),
        labels,
        install_url,
        is_accessible,
        is_enabled,
        plugin_display_names,
    }
}

/// Converts metadata-only connector data into the app-server wire type.
///
/// Keeping this separate from app_info_to_api makes it impossible for app/read to accidentally
/// expose full runtime tool state from the broader app/list path.
pub(crate) fn connector_metadata_to_api(metadata: ConnectorMetadata) -> ApiConnectorMetadata {
    let ConnectorMetadata {
        id,
        name,
        description,
        icon_url,
        icon_url_dark,
        distribution_channel,
        tool_summaries,
    } = metadata;
    let install_url = Some(connector_install_url(&name, &id));
    ApiConnectorMetadata {
        id,
        name,
        description,
        icon_url,
        icon_url_dark,
        distribution_channel,
        install_url,
        plugin_display_names: Vec::new(),
        tool_summaries: tool_summaries.map(|tools| {
            tools
                .into_iter()
                .map(|tool| {
                    let ConnectorToolSummary {
                        name,
                        title,
                        description,
                        is_enabled,
                        disabled_reason,
                        is_read_only,
                    } = tool;
                    ApiAppToolSummary {
                        name,
                        title,
                        description,
                        is_enabled,
                        disabled_reason,
                        is_read_only,
                    }
                })
                .collect()
        }),
    }
}

fn app_branding_to_api(branding: AppBranding) -> ApiAppBranding {
    let AppBranding {
        category,
        developer,
        website,
        privacy_policy,
        terms_of_service,
        is_discoverable_app,
    } = branding;
    ApiAppBranding {
        category,
        developer,
        website,
        privacy_policy,
        terms_of_service,
        is_discoverable_app,
    }
}

fn app_review_to_api(review: AppReview) -> ApiAppReview {
    let AppReview { status } = review;
    ApiAppReview { status }
}

fn app_screenshot_to_api(screenshot: AppScreenshot) -> ApiAppScreenshot {
    let AppScreenshot {
        url,
        file_id,
        user_prompt,
    } = screenshot;
    ApiAppScreenshot {
        url,
        file_id,
        user_prompt,
    }
}

fn app_metadata_to_api(metadata: AppMetadata) -> ApiAppMetadata {
    let AppMetadata {
        review,
        categories,
        sub_categories,
        seo_description,
        screenshots,
        developer,
        version,
        version_id,
        version_notes,
        first_party_requires_install,
        show_in_composer_when_unlinked,
    } = metadata;
    ApiAppMetadata {
        review: review.map(app_review_to_api),
        categories,
        sub_categories,
        seo_description,
        screenshots: screenshots
            .map(|screenshots| screenshots.into_iter().map(app_screenshot_to_api).collect()),
        developer,
        version,
        version_id,
        version_notes,
        first_party_requires_install,
        show_in_composer_when_unlinked,
    }
}
