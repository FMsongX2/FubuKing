// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_core::config::Config;
use atlas_engine_extension_api::ExtensionFuture;
use atlas_engine_extension_api::ExtensionRegistryBuilder;
use atlas_engine_extension_api::McpServerContribution;
use atlas_engine_extension_api::McpServerContributionContext;
use atlas_engine_extension_api::McpServerContributor;
use atlas_engine_mcp::ATLAS_APPS_MCP_SERVER_NAME;
use atlas_engine_mcp::hosted_plugin_runtime_mcp_server_config;

mod executor_plugin;

struct HostedPluginRuntimeExtension;

impl McpServerContributor<Config> for HostedPluginRuntimeExtension {
    fn id(&self) -> &'static str {
        "hosted_plugin_runtime"
    }

    fn contribute<'a>(
        &'a self,
        context: McpServerContributionContext<'a, Config>,
    ) -> ExtensionFuture<'a, Vec<McpServerContribution>> {
        Box::pin(async move {
            let config = context.config();
            let name = ATLAS_APPS_MCP_SERVER_NAME.to_string();
            if !config.features.enabled(atlas_engine_features::Feature::Apps) {
                return vec![McpServerContribution::Remove { name }];
            }

            vec![McpServerContribution::Set {
                name,
                config: Box::new(hosted_plugin_runtime_mcp_server_config(
                    &config.chatgpt_base_url,
                    config.apps_mcp_product_sku.as_deref(),
                    context.originator(),
                )),
            }]
        })
    }
}

pub fn install(builder: &mut ExtensionRegistryBuilder<Config>) {
    builder.mcp_server_contributor(std::sync::Arc::new(HostedPluginRuntimeExtension));
}

/// Installs discovery for MCP servers declared by thread-selected executor plugins.
pub fn install_executor_plugins(
    builder: &mut ExtensionRegistryBuilder<Config>,
    environment_manager: std::sync::Arc<atlas_engine_exec_server::EnvironmentManager>,
) {
    builder.mcp_server_contributor(std::sync::Arc::new(
        executor_plugin::SelectedExecutorPluginMcpContributor::new(environment_manager),
    ));
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
