// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::sync::Arc;

use atlas_engine_core::config::Config;
use atlas_engine_extension_api::ConfigContributor;
use atlas_engine_extension_api::ContextContributor;
use atlas_engine_extension_api::ExtensionData;
use atlas_engine_extension_api::ExtensionFuture;
use atlas_engine_extension_api::ExtensionRegistryBuilder;
use atlas_engine_extension_api::PromptFragment;
use atlas_engine_extension_api::ThreadLifecycleContributor;
use atlas_engine_extension_api::ThreadStartInput;
use atlas_engine_extension_api::ToolContributor;
use atlas_engine_features::Feature;
use atlas_engine_otel::MetricsClient;
use atlas_engine_utils_absolute_path::AbsolutePathBuf;

use crate::local::LocalMemoriesBackend;
use crate::prompts::build_memory_tool_developer_instructions;
use crate::tools;

/// Contributes Atlas Agent memory read-path prompt context and memory read tools.
#[derive(Clone, Default)]
pub(crate) struct MemoriesExtension {
    metrics_client: Option<MetricsClient>,
}

impl MemoriesExtension {
    fn new(metrics_client: Option<MetricsClient>) -> Self {
        Self { metrics_client }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MemoriesExtensionConfig {
    pub(crate) enabled: bool,
    pub(crate) dedicated_tools: bool,
    pub(crate) atlas_agent_home: AbsolutePathBuf,
}

impl MemoriesExtensionConfig {
    fn from_config(config: &Config) -> Self {
        Self {
            enabled: config.features.enabled(Feature::MemoryTool) && config.memories.use_memories,
            dedicated_tools: config.memories.dedicated_tools,
            atlas_agent_home: config.atlas_agent_home.clone(),
        }
    }
}

impl ContextContributor for MemoriesExtension {
    fn contribute_thread_context<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<PromptFragment>> + Send + 'a>> {
        Box::pin(async move {
            let Some(config) = thread_store.get::<MemoriesExtensionConfig>() else {
                return Vec::new();
            };
            if !config.enabled {
                return Vec::new();
            }

            build_memory_tool_developer_instructions(&config.atlas_agent_home)
                .await
                .map(PromptFragment::developer_policy)
                .into_iter()
                .collect()
        })
    }
}

impl ThreadLifecycleContributor<Config> for MemoriesExtension {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input
                .thread_store
                .insert(MemoriesExtensionConfig::from_config(input.config));
        })
    }
}

impl ConfigContributor<Config> for MemoriesExtension {
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &Config,
        new_config: &Config,
    ) {
        thread_store.insert(MemoriesExtensionConfig::from_config(new_config));
    }
}

impl ToolContributor for MemoriesExtension {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn atlas_engine_extension_api::ToolExecutor<atlas_engine_extension_api::ToolCall>>> {
        let Some(config) = thread_store.get::<MemoriesExtensionConfig>() else {
            return Vec::new();
        };
        if !config.enabled || !config.dedicated_tools {
            return Vec::new();
        }

        tools::memory_tools(
            LocalMemoriesBackend::from_atlas_agent_home(&config.atlas_agent_home),
            self.metrics_client.clone(),
        )
    }
}

/// Installs the memories extension contributors into the extension registry.
pub fn install(
    registry: &mut ExtensionRegistryBuilder<Config>,
    metrics_client: Option<MetricsClient>,
) {
    let extension = Arc::new(MemoriesExtension::new(metrics_client));
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.prompt_contributor(extension.clone());
    registry.tool_contributor(extension);
}
