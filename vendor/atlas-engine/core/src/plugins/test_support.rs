// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use crate::config::CONFIG_TOML_FILE;
use crate::config::ConfigBuilder;
use std::fs;
use std::path::Path;

use atlas_engine_core_plugins::OPENAI_API_CURATED_MARKETPLACE_NAME;

pub(crate) const TEST_CURATED_PLUGIN_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

pub(crate) fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("file should have a parent")).unwrap();
    fs::write(path, contents).unwrap();
}

pub(crate) fn write_curated_plugin(root: &Path, plugin_name: &str) {
    let plugin_root = root.join("plugins").join(plugin_name);
    write_file(
        &plugin_root.join(".atlas-agent-plugin/plugin.json"),
        &format!(
            r#"{{
  "name": "{plugin_name}",
  "description": "Plugin that includes skills, MCP servers, and app connectors"
}}"#
        ),
    );
    write_file(
        &plugin_root.join("skills/SKILL.md"),
        "---\nname: sample\ndescription: sample\n---\n",
    );
    write_file(
        &plugin_root.join(".mcp.json"),
        r#"{
  "mcpServers": {
    "sample-docs": {
      "type": "http",
      "url": "https://sample.example/mcp"
    }
  }
}"#,
    );
    write_file(
        &plugin_root.join(".app.json"),
        r#"{
  "apps": {
    "calendar": {
      "id": "connector_calendar"
    }
  }
}"#,
    );
}

pub(crate) fn write_openai_api_curated_marketplace(root: &Path, plugin_names: &[&str]) {
    let plugins = plugin_names
        .iter()
        .map(|plugin_name| {
            format!(
                r#"{{
      "name": "{plugin_name}",
      "source": {{
        "source": "local",
        "path": "./plugins/{plugin_name}"
      }}
    }}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    write_file(
        &root.join(".agents/plugins/api_marketplace.json"),
        &format!(
            r#"{{
  "name": "{OPENAI_API_CURATED_MARKETPLACE_NAME}",
  "plugins": [
{plugins}
  ]
}}"#
        ),
    );
    for plugin_name in plugin_names {
        write_curated_plugin(root, plugin_name);
    }
}

pub(crate) fn write_curated_plugin_sha(atlas_agent_home: &Path) {
    write_curated_plugin_sha_with(atlas_agent_home, TEST_CURATED_PLUGIN_SHA);
}

pub(crate) fn write_curated_plugin_sha_with(atlas_agent_home: &Path, sha: &str) {
    write_file(&atlas_agent_home.join(".tmp/plugins.sha"), &format!("{sha}\n"));
}

pub(crate) fn write_plugins_feature_config(atlas_agent_home: &Path) {
    write_file(
        &atlas_agent_home.join(CONFIG_TOML_FILE),
        r#"[features]
plugins = true
"#,
    );
}

pub(crate) async fn load_plugins_config(atlas_agent_home: &Path) -> crate::config::Config {
    ConfigBuilder::default()
        .atlas_agent_home(atlas_agent_home.to_path_buf())
        .fallback_cwd(Some(atlas_agent_home.to_path_buf()))
        .build()
        .await
        .expect("config should load")
}
