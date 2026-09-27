//! Quotatlas accounts: extra logins of a supported agent, each installed as an
//! agent entry of its own (ADR Q-0002).
//!
//! An account is a registry entry with a `base` (the agent it runs), a `label`
//! and an environment that points the agent's CLI at a private profile home.
//! The CLI keeps its own settings and credentials there; Quotatlas creates the
//! directory and never reads what the CLI writes into it.
//!
//! This module is where Quotatlas branches on agent identity, because the
//! profile variable is a property of each CLI: Claude Code reads
//! `CLAUDE_CONFIG_DIR`, the Codex CLI reads `CODEX_HOME`. Removing an account
//! is the ordinary `acp_registry_uninstall`; the profile home is kept so a
//! reinstalled account finds its login again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atlas_acp_thread::AgentId;
use atlas_agent_store::{AgentServerSettings, AllAgentServersSettings};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::agent_host::AgentHost;

/// Longest label accepted, in characters. Long enough for "client-a staging".
const MAX_LABEL_CHARS: usize = 40;

/// Directory under the app config dir holding one profile home per account.
const ACCOUNTS_DIR: &str = "accounts";

/// Which CLI an account's base agent drives, and therefore which variable
/// selects its profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccountProvider {
    Claude,
    Codex,
}

impl AccountProvider {
    /// The provider for a base agent id, or `None` when accounts are not
    /// supported for it. Registry ids first, then the ids a detected install
    /// uses.
    pub fn for_base(base: &str) -> Option<Self> {
        match base {
            "claude-acp" | "claude-code" => Some(Self::Claude),
            "codex-acp" | "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    /// The environment that points the CLI at `home`.
    ///
    /// Claude also gets `ANTHROPIC_API_KEY` blanked, mirroring the upstream
    /// env quirk for its registry id: with a key present the CLI bills the key
    /// instead of the subscription the account logged in with, and the quirk
    /// is keyed by agent id, so it does not reach an account entry by itself.
    pub fn profile_env(self, home: &Path) -> HashMap<String, String> {
        let home = home.to_string_lossy().into_owned();
        match self {
            Self::Claude => HashMap::from([
                ("CLAUDE_CONFIG_DIR".to_string(), home),
                ("ANTHROPIC_API_KEY".to_string(), String::new()),
            ]),
            Self::Codex => HashMap::from([("CODEX_HOME".to_string(), home)]),
        }
    }
}

/// One account as the settings UI lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    /// The agent id sessions run under, e.g. `claude-acp@work`.
    pub id: String,
    pub base_agent_id: String,
    pub label: String,
    pub provider: AccountProvider,
    /// Where the CLI keeps this account's settings and login.
    pub profile_home: String,
    /// The name pickers show, when the registry has resolved the base agent.
    pub display_name: Option<String>,
}

/// Lowercase ASCII letters, digits and single dashes, for the id suffix.
fn slug(label: &str) -> String {
    let mut out = String::new();
    for ch in label.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    let short: String = trimmed.chars().take(32).collect();
    if short.is_empty() {
        "account".to_string()
    } else {
        short.trim_end_matches('-').to_string()
    }
}

/// `base@slug`, with a numeric suffix when that key is already installed.
fn unique_account_id(base: &str, label: &str, installed: &AllAgentServersSettings) -> String {
    let stem = format!("{base}@{}", slug(label));
    if !installed.contains_key(&stem) {
        return stem;
    }
    (2..)
        .map(|n| format!("{stem}-{n}"))
        .find(|candidate| !installed.contains_key(candidate))
        .expect("an unbounded range always yields a free id")
}

/// Trimmed, non-empty and short enough to fit a picker row.
fn validate_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("an account needs a name".to_string());
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return Err(format!("keep the account name under {MAX_LABEL_CHARS} characters"));
    }
    Ok(label.to_string())
}

fn profile_home(config_dir: &Path, account_id: &str) -> PathBuf {
    config_dir
        .join(ACCOUNTS_DIR)
        .join(atlas_agent_store::sanitize_path_component(account_id))
}

/// Create the profile home, private to the user: the CLI writes its login
/// there.
fn create_profile_home(home: &Path) -> Result<(), String> {
    std::fs::create_dir_all(home).map_err(|e| format!("creating {}: {e}", home.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(home, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("securing {}: {e}", home.display()))?;
    }
    Ok(())
}

fn account_view(host: &AgentHost, id: &str, entry: &AgentServerSettings) -> Option<AccountView> {
    let label = entry.label()?.to_string();
    let base = entry.registry_id(id)?.to_string();
    let provider = AccountProvider::for_base(&base)?;
    let home = match provider {
        AccountProvider::Claude => entry.env().get("CLAUDE_CONFIG_DIR"),
        AccountProvider::Codex => entry.env().get("CODEX_HOME"),
    }?
    .clone();
    Some(AccountView {
        id: id.to_string(),
        base_agent_id: base,
        label,
        provider,
        profile_home: home,
        display_name: host.store().agent_display_name(&AgentId::new(id)),
    })
}

/// Every account entry in the installed map, sorted by display order.
#[tauri::command]
pub fn accounts_list(app: AppHandle) -> Vec<AccountView> {
    let host = app.state::<Arc<AgentHost>>().inner().clone();
    let settings = host.store().settings();
    let mut accounts: Vec<AccountView> = settings
        .iter()
        .filter_map(|(id, entry)| account_view(&host, id, entry))
        .collect();
    accounts.sort_by(|a, b| {
        a.base_agent_id
            .cmp(&b.base_agent_id)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    accounts
}

/// Install a new account of `base_agent_id` named `label`.
///
/// Creates the profile home and writes the entry; nothing is spawned. The
/// first session on the account starts the CLI, which asks the user to log in
/// through the agent's own sign-in flow.
#[tauri::command]
pub async fn accounts_create(
    base_agent_id: String,
    label: String,
    app: AppHandle,
) -> Result<AccountView, String> {
    let label = validate_label(&label)?;
    let provider = AccountProvider::for_base(&base_agent_id)
        .ok_or_else(|| "accounts are available for the Claude and Codex agents".to_string())?;

    let host = app.state::<Arc<AgentHost>>().inner().clone();
    host.registry().refresh_if_stale().await;
    if host.registry().agent(&base_agent_id).is_none() {
        return Err(format!("{base_agent_id} is not in the agent registry"));
    }

    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let installed = host.store().settings();
    let id = unique_account_id(&base_agent_id, &label, &installed);
    let home = profile_home(&config_dir, &id);
    create_profile_home(&home)?;

    let entry = AgentServerSettings::account(&base_agent_id, &label, provider.profile_env(&home));
    let settings = super::registry::with_entry(&host, &id, entry.clone());
    super::registry::persist(&host, &super::registry::app_data_dir(&app), settings).await?;
    super::catalog::emit_catalog_changed(&app, "install");

    account_view(&host, &id, &entry).ok_or_else(|| "the account was saved but could not be read back".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_are_recognised_by_registry_and_detected_ids() {
        assert_eq!(AccountProvider::for_base("claude-acp"), Some(AccountProvider::Claude));
        assert_eq!(AccountProvider::for_base("claude-code"), Some(AccountProvider::Claude));
        assert_eq!(AccountProvider::for_base("codex-acp"), Some(AccountProvider::Codex));
        assert_eq!(AccountProvider::for_base("gemini"), None);
    }

    #[test]
    fn claude_profiles_select_the_config_dir_and_bill_the_subscription() {
        let env = AccountProvider::Claude.profile_env(Path::new("/p/work"));
        assert_eq!(env.get("CLAUDE_CONFIG_DIR").map(String::as_str), Some("/p/work"));
        assert_eq!(env.get("ANTHROPIC_API_KEY").map(String::as_str), Some(""));
        let env = AccountProvider::Codex.profile_env(Path::new("/p/side"));
        assert_eq!(env.get("CODEX_HOME").map(String::as_str), Some("/p/side"));
        assert_eq!(env.len(), 1);
    }

    #[test]
    fn slugs_are_short_lowercase_and_dash_separated() {
        assert_eq!(slug("Client A  Staging!"), "client-a-staging");
        assert_eq!(slug("  --  "), "account");
        assert_eq!(slug("\u{4e16}\u{754c}"), "account");
        assert_eq!(slug(&"x".repeat(80)).len(), 32);
    }

    #[test]
    fn account_ids_never_collide_with_an_installed_entry() {
        let mut installed = AllAgentServersSettings::default();
        assert_eq!(unique_account_id("claude-acp", "Work", &installed), "claude-acp@work");
        installed.insert("claude-acp@work".into(), AgentServerSettings::registry());
        assert_eq!(unique_account_id("claude-acp", "work", &installed), "claude-acp@work-2");
        installed.insert("claude-acp@work-2".into(), AgentServerSettings::registry());
        assert_eq!(unique_account_id("claude-acp", "work", &installed), "claude-acp@work-3");
    }

    #[test]
    fn labels_are_trimmed_and_bounded() {
        assert_eq!(validate_label("  work ").unwrap(), "work");
        assert!(validate_label("   ").is_err());
        assert!(validate_label(&"a".repeat(MAX_LABEL_CHARS + 1)).is_err());
    }

    #[test]
    fn profile_homes_live_under_the_app_config_dir() {
        let home = profile_home(Path::new("/cfg"), "claude-acp@work");
        assert!(home.starts_with("/cfg/accounts"));
        assert!(!home.to_string_lossy().contains(".."));
    }

    #[cfg(unix)]
    #[test]
    fn profile_homes_are_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("accounts").join("claude-acp@work");
        create_profile_home(&home).unwrap();
        let mode = std::fs::metadata(&home).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
