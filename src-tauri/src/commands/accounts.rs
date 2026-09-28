//! FubuKing accounts: extra logins of a supported agent, each installed as an
//! agent entry of its own (ADR Q-0002).
//!
//! An account is a registry entry with a `base` (the agent it runs), a `label`
//! and an environment that points the agent's CLI at a private profile home.
//! The CLI keeps its own settings and credentials there; FubuKing creates the
//! directory and never reads what the CLI writes into it.
//!
//! This module is where FubuKing branches on agent identity, because the
//! profile variable is a property of each CLI: Claude Code reads
//! `CLAUDE_CONFIG_DIR`, the Codex CLI reads `CODEX_HOME`. A Claude profile
//! also gets the status line script the quota service reads limits through
//! (`crate::quota::claude`). Removing an account
//! is the ordinary `acp_registry_uninstall`; the profile home is kept so a
//! reinstalled account finds its login again.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use atlas_acp_thread::AgentId;
use atlas_agent_store::{AgentServerSettings, AllAgentServersSettings};
use fubuking::accounts::{slug, ACCOUNTS_DIR};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::agent_host::{AgentHost, AuthMethodWire};

/// Which CLI an account belongs to. Shared with the `fubuking` CLI, which
/// finds the same accounts by their profile directories.
pub use fubuking::accounts::Provider as AccountProvider;

/// Longest label accepted, in characters. Long enough for "client-a staging".
const MAX_LABEL_CHARS: usize = 40;

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
    let (provider, home) = profile_of(&base, entry)?;
    Some(AccountView {
        id: id.to_string(),
        base_agent_id: base,
        label,
        provider,
        profile_home: home,
        display_name: host.store().agent_display_name(&AgentId::new(id)),
    })
}

/// The provider of an entry based on `base` and the profile home its
/// environment points the CLI at.
fn profile_of(base: &str, entry: &AgentServerSettings) -> Option<(AccountProvider, String)> {
    let provider = AccountProvider::for_base(base)?;
    let home = match provider {
        AccountProvider::Claude => entry.env().get("CLAUDE_CONFIG_DIR"),
        AccountProvider::Codex => entry.env().get("CODEX_HOME"),
    }?;
    Some((provider, home.clone()))
}

/// The variables that select the profile of the account stored under `id`,
/// for its sign-in line; none for an entry that is not an account. Only the
/// profile variables, never the rest of the entry's environment: the line is
/// shown, copied and typed into a shell.
fn sign_in_vars(id: &str, entry: &AgentServerSettings) -> Vec<(String, String)> {
    let profile = entry.label().and(entry.registry_id(id)).and_then(|base| profile_of(base, entry));
    let Some((provider, home)) = profile else {
        return Vec::new();
    };
    let mut vars: Vec<(String, String)> = provider.profile_env(Path::new(&home)).into_iter().collect();
    vars.sort();
    vars
}

/// Give an account's runnable sign-in methods the variables that select its
/// profile. The login runs in the user's own shell, which has none of the
/// agent's environment: without them it signs in the default profile, which
/// leaves the account signed out and replaces the default login.
pub(crate) fn carry_profile(host: &AgentHost, agent_id: &str, methods: &mut [AuthMethodWire]) {
    let vars = host
        .store()
        .settings()
        .get(agent_id)
        .map(|entry| sign_in_vars(agent_id, entry))
        .unwrap_or_default();
    for method in methods.iter_mut().filter(|method| method.terminal_command.is_some()) {
        method.terminal_env.extend(vars.iter().cloned());
    }
}

/// Every account entry in the installed map, sorted by base agent, then
/// label. Shared with the quota service, which reads the same accounts.
pub(crate) fn installed_accounts(host: &AgentHost) -> Vec<AccountView> {
    let settings = host.store().settings();
    let mut accounts: Vec<AccountView> = settings
        .iter()
        .filter_map(|(id, entry)| account_view(host, id, entry))
        .collect();
    accounts.sort_by(|a, b| {
        a.base_agent_id
            .cmp(&b.base_agent_id)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    accounts
}

/// Every account entry in the installed map, after adopting the profiles
/// `fubuking login` made.
#[tauri::command]
pub async fn accounts_list(app: AppHandle) -> Vec<AccountView> {
    let host = app.state::<Arc<AgentHost>>().inner().clone();
    adopt_cli_profiles(&app, &host).await;
    installed_accounts(&host)
}

/// Give each profile `fubuking login` made an entry, once, so an account made
/// in the CLI is an agent here too. Only a profile still carrying the CLI's
/// adoption marker counts: an account removed here keeps its profile home and
/// must stay removed. One adoption runs at a time, so the settings list and a
/// quota refresh cannot both adopt a profile; a call that finds one running
/// leaves the work to it. A provider whose base agent the registry does not
/// list yet is left for a later call.
pub(crate) async fn adopt_cli_profiles(app: &AppHandle, host: &Arc<AgentHost>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ADOPTING: AtomicBool = AtomicBool::new(false);
    /// Clears the flag however adoption ends.
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            ADOPTING.store(false, Ordering::Release);
        }
    }
    if ADOPTING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return;
    }
    let _running = Running;
    let Ok(config_dir) = app.path().app_config_dir() else { return };
    let dir = config_dir.join(ACCOUNTS_DIR);
    let known: Vec<String> = installed_accounts(host).into_iter().map(|account| account.profile_home).collect();
    let mut adopted = false;
    for provider in AccountProvider::ALL {
        let base = provider.base();
        for profile in fubuking::accounts::list_in(Some(&dir), provider) {
            let Some(home) = profile.home else { continue };
            let marker = home.join(fubuking::accounts::ADOPT_MARKER);
            if !marker.exists() || host.registry().agent(base).is_none() {
                continue;
            }
            if !known.contains(&home.to_string_lossy().into_owned()) {
                let id = unique_account_id(base, &profile.label, &host.store().settings());
                let entry = AgentServerSettings::account(base, &profile.label, provider.profile_env(&home));
                let settings = super::registry::with_entry(host, &id, entry);
                if let Err(e) = super::registry::persist(host, &super::registry::app_data_dir(app), settings).await {
                    tracing::warn!(target: "fubuking::accounts", "adopting {}: {e}", home.display());
                    continue;
                }
                adopted = true;
            }
            let _ = std::fs::remove_file(&marker);
        }
    }
    if adopted {
        super::catalog::emit_catalog_changed(app, "install");
    }
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
    if provider == AccountProvider::Claude {
        // How the quota service reads this account's limits (ADR Q-0002).
        crate::quota::claude::install_statusline(&home)
            .map_err(|e| format!("preparing {}: {e}", home.display()))?;
    }

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

    #[test]
    fn an_accounts_sign_in_selects_its_profile_and_carries_nothing_else() {
        let mut env = AccountProvider::Claude.profile_env(Path::new("/p/claude-acp-work"));
        env.insert("HTTPS_PROXY".into(), "http://proxy".into());
        let entry = AgentServerSettings::account("claude-acp", "work", env);
        assert_eq!(
            sign_in_vars("claude-acp@work", &entry),
            [
                ("ANTHROPIC_API_KEY".to_string(), String::new()),
                ("CLAUDE_CONFIG_DIR".to_string(), "/p/claude-acp-work".to_string()),
            ]
        );
        let codex = AgentServerSettings::account(
            "codex-acp",
            "side",
            AccountProvider::Codex.profile_env(Path::new("/p/codex-acp-side")),
        );
        assert_eq!(
            sign_in_vars("codex-acp@side", &codex),
            [("CODEX_HOME".to_string(), "/p/codex-acp-side".to_string())]
        );
        assert!(sign_in_vars("claude-acp", &AgentServerSettings::registry()).is_empty());
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
