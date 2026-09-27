// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::process::Command;

use anyhow::Result;
use tempfile::TempDir;

#[test]
fn strict_config_rejects_unknown_config_fields_for_standalone_app_server() -> Result<()> {
    let atlas_agent_home = TempDir::new()?;
    std::fs::write(
        atlas_agent_home.path().join("config.toml"),
        r#"
foo = "bar"
"#,
    )?;

    let output = Command::new(atlas_engine_utils_cargo_bin::cargo_bin("atlas-engine-app-server")?)
        .env("ATLAS_AGENT_HOME", atlas_agent_home.path())
        .env(
            "ATLAS_AGENT_APP_SERVER_MANAGED_CONFIG_PATH",
            atlas_agent_home.path().join("managed_config.toml"),
        )
        .args(["--strict-config", "--listen", "off"])
        .output()?;

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("unknown configuration field `foo`"),
        "expected strict config error in stderr, got: {stderr}"
    );

    Ok(())
}

#[test]
fn managed_auth_requirements_fail_closed_for_standalone_app_server() -> Result<()> {
    for requirements in [
        "allowed_login_methods = []\n",
        "allowed_login_methods = [\"chatgpt\"]\nallowed_chatgpt_workspaces = []\n",
    ] {
        let atlas_agent_home = TempDir::new()?;
        std::fs::write(atlas_agent_home.path().join("requirements.toml"), requirements)?;

        let output = Command::new(atlas_engine_utils_cargo_bin::cargo_bin("atlas-engine-app-server")?)
            .env("ATLAS_AGENT_HOME", atlas_agent_home.path())
            .env(
                "ATLAS_AGENT_APP_SERVER_MANAGED_CONFIG_PATH",
                atlas_agent_home.path().join("managed_config.toml"),
            )
            .args(["--listen", "off"])
            .output()?;

        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr)?;
        assert!(
            stderr.contains("authentication requirements do not permit any usable login method"),
            "expected managed authentication error in stderr, got: {stderr}"
        );
        assert!(
            !stderr.contains("using defaults"),
            "managed authentication requirements must not fall back to defaults"
        );
    }

    Ok(())
}
