//! Accounts: the logins Quotatlas can run an agent CLI under.
//!
//! Each CLI's default login is one account. Every other account is a profile
//! home, a `CLAUDE_CONFIG_DIR` or `CODEX_HOME`, at `<app config dir>/accounts/<id>`
//! where the id is `<base agent id>@<label slug>` with `@` made path-safe. The
//! directory listing is the registry, so the desktop app and the CLI find the
//! same accounts.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The desktop app's bundle identifier, which names its config and data dirs.
pub const IDENTIFIER: &str = "io.github.fmsongx2.quotatlas";
/// Directory under the app config dir holding one profile home per account.
pub const ACCOUNTS_DIR: &str = "accounts";
/// The label of each CLI's own login.
pub const DEFAULT_LABEL: &str = "default";

/// Which CLI an account belongs to, and therefore which variable selects its
/// profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Claude, Provider::Codex];

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

    /// The CLI's executable name, which is also how the CLI is named to users.
    pub fn program(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// The registry id the desktop app bases this provider's accounts on.
    fn base(self) -> &'static str {
        match self {
            Self::Claude => "claude-acp",
            Self::Codex => "codex-acp",
        }
    }

    /// The variable that points the CLI at a profile home.
    fn home_var(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_CONFIG_DIR",
            Self::Codex => "CODEX_HOME",
        }
    }

    /// The environment that points the CLI at `home`.
    ///
    /// Claude also gets `ANTHROPIC_API_KEY` blanked: with a key present the
    /// CLI bills the key instead of the subscription the account logged in
    /// with.
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

    /// Where the CLI keeps its default login: the variable when the user set
    /// it, else `~/.claude` or `~/.codex`.
    pub fn default_home(self) -> Option<PathBuf> {
        std::env::var_os(self.home_var())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(format!(".{}", self.program()))))
    }
}

impl std::str::FromStr for Provider {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.program() == name)
            .ok_or_else(|| format!("`{name}` is not `claude` or `codex`"))
    }
}

/// One login an agent can run under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// `default`, or the profile directory's name, e.g. `claude-acp-work`.
    pub id: String,
    pub provider: Provider,
    /// What `--account` matches: `default`, or the slug the account was named with.
    pub label: String,
    /// The profile home; `None` for the default login.
    pub home: Option<PathBuf>,
}

impl Account {
    pub fn default_for(provider: Provider) -> Self {
        Self { id: DEFAULT_LABEL.to_string(), provider, label: DEFAULT_LABEL.to_string(), home: None }
    }

    /// The directory the CLI reads and writes for this account.
    pub fn cli_home(&self) -> Option<PathBuf> {
        self.home.clone().or_else(|| self.provider.default_home())
    }

    /// The variables that select this account. Empty for the default login,
    /// which runs with the environment as it is.
    pub fn env(&self) -> HashMap<String, String> {
        self.home.as_deref().map(|home| self.provider.profile_env(home)).unwrap_or_default()
    }
}

impl std::fmt::Display for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} account `{}`", self.provider.program(), self.label)
    }
}

/// The desktop app's config dir, the same one Tauri resolves.
pub fn app_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join(IDENTIFIER))
}

/// The desktop app's data dir, the same one Tauri resolves.
pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(IDENTIFIER))
}

pub fn accounts_dir() -> Option<PathBuf> {
    app_config_dir().map(|dir| dir.join(ACCOUNTS_DIR))
}

/// The default login, then every profile of `provider` by label.
pub fn list(provider: Provider) -> Vec<Account> {
    list_in(accounts_dir().as_deref(), provider)
}

fn list_in(dir: Option<&Path>, provider: Provider) -> Vec<Account> {
    let mut profiles: Vec<Account> = dir
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let id = entry.file_name().into_string().ok()?;
            let (found, label) = parse_profile(&id)?;
            (found == provider).then(|| Account { id, provider, label, home: Some(entry.path()) })
        })
        .collect();
    profiles.sort_by(|a, b| a.label.cmp(&b.label));
    std::iter::once(Account::default_for(provider)).chain(profiles).collect()
}

/// `(provider, label)` for a profile directory name.
fn parse_profile(name: &str) -> Option<(Provider, String)> {
    // Longer bases first: `codex-acp-work` is an account of `codex-acp`.
    const BASES: [&str; 4] = ["claude-acp", "claude-code", "codex-acp", "codex"];
    BASES.into_iter().find_map(|base| {
        let label = name.strip_prefix(base)?.strip_prefix('-').filter(|label| !label.is_empty())?;
        Some((Provider::for_base(base)?, label.to_string()))
    })
}

/// The account called `wanted`, by label or by id.
pub fn find<'a>(accounts: &'a [Account], wanted: &str) -> Option<&'a Account> {
    accounts.iter().find(|account| account.label == wanted || account.id == wanted)
}

/// Lowercase ASCII letters, digits and single dashes, for the id suffix.
pub fn slug(label: &str) -> String {
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

/// The profile for `label`, created private to the user when it does not
/// exist yet: the CLI writes its login there.
pub fn create(provider: Provider, label: &str) -> io::Result<Account> {
    let dir = accounts_dir().ok_or_else(|| io::Error::other("no config directory on this system"))?;
    create_in(&dir, provider, label)
}

fn create_in(dir: &Path, provider: Provider, label: &str) -> io::Result<Account> {
    let label = slug(label);
    if label == DEFAULT_LABEL {
        return Err(io::Error::other("`default` is the CLI's own login; pick another name"));
    }
    let id = format!("{}-{label}", provider.base());
    let home = dir.join(&id);
    std::fs::create_dir_all(&home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))?;
    }
    if provider == Provider::Claude {
        crate::quota::claude::install_statusline(&home)?;
    }
    Ok(Account { id, provider, label, home: Some(home) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_directories_name_their_provider_and_label() {
        assert_eq!(parse_profile("claude-acp-work"), Some((Provider::Claude, "work".into())));
        assert_eq!(parse_profile("codex-acp-client-a"), Some((Provider::Codex, "client-a".into())));
        assert_eq!(parse_profile("codex-side"), Some((Provider::Codex, "side".into())));
        assert_eq!(parse_profile("claude-acp-"), None);
        assert_eq!(parse_profile("gemini-work"), None);
    }

    #[test]
    fn listing_puts_the_default_login_first_and_keeps_providers_apart() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["claude-acp-work", "claude-acp-alt", "codex-acp-work", "notes"] {
            std::fs::create_dir(dir.path().join(name)).unwrap();
        }
        let labels: Vec<String> =
            list_in(Some(dir.path()), Provider::Claude).into_iter().map(|a| a.label).collect();
        assert_eq!(labels, ["default", "alt", "work"]);
        assert_eq!(list_in(None, Provider::Codex), vec![Account::default_for(Provider::Codex)]);
    }

    #[test]
    fn a_created_account_is_found_by_the_next_listing() {
        let dir = tempfile::tempdir().unwrap();
        let made = create_in(dir.path(), Provider::Codex, "Client A").unwrap();
        assert_eq!((made.id.as_str(), made.label.as_str()), ("codex-acp-client-a", "client-a"));
        let listed = list_in(Some(dir.path()), Provider::Codex);
        assert_eq!(find(&listed, "client-a"), Some(&made));
        assert!(create_in(dir.path(), Provider::Codex, "Default").is_err());
    }

    #[test]
    fn a_profile_selects_itself_and_the_default_login_changes_nothing() {
        let work = Account {
            id: "claude-acp-work".into(),
            provider: Provider::Claude,
            label: "work".into(),
            home: Some(PathBuf::from("/p/work")),
        };
        assert_eq!(work.env()["CLAUDE_CONFIG_DIR"], "/p/work");
        assert_eq!(work.env()["ANTHROPIC_API_KEY"], "");
        assert!(Account::default_for(Provider::Codex).env().is_empty());
    }

    #[test]
    fn providers_are_recognised_by_registry_and_detected_ids() {
        assert_eq!(Provider::for_base("claude-acp"), Some(Provider::Claude));
        assert_eq!(Provider::for_base("claude-code"), Some(Provider::Claude));
        assert_eq!(Provider::for_base("codex-acp"), Some(Provider::Codex));
        assert_eq!(Provider::for_base("gemini"), None);
        assert_eq!("codex".parse::<Provider>(), Ok(Provider::Codex));
        assert!("gemini".parse::<Provider>().is_err());
    }

    #[test]
    fn codex_profiles_select_only_their_home() {
        let env = Provider::Codex.profile_env(Path::new("/p/side"));
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
}
