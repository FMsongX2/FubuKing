//! Quotatlas quota: per-account usage limits (ADR Q-0002).
//!
//! Every account the app knows about (the default Claude Code and Codex
//! logins, plus each account entry) gets one `AccountQuota`. Figures come only
//! from channels the official CLIs expose; the readers live in the `quotatlas`
//! crate, shared with the CLI. The service refreshes on a timer and on demand,
//! keeps the last snapshot for the UI and broadcasts each new one as
//! `atlas:quota-changed`.

pub use quotatlas::quota::{claude, codex, QuotaWindow};
use quotatlas::quota::now_secs;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::accounts::{installed_accounts, AccountProvider};
use crate::commands::agent_host::AgentHost;

/// Broadcast with the full snapshot after every refresh.
pub const QUOTA_CHANGED_EVENT: &str = "atlas:quota-changed";
/// How often the service refreshes on its own. Five minutes keeps a reset
/// countdown honest without polling the CLIs hard.
const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Let the installed map and PATH settle after launch before the first read.
const FIRST_POLL_DELAY: Duration = Duration::from_secs(8);

/// Whether an account's figures can be shown, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuotaStatus {
    /// Figures are current as of `updated_at`.
    Ok,
    /// The account works but has not reported figures yet.
    Pending,
    /// The CLI has no login for this account.
    SignedOut,
    /// Quotatlas does not read this account, by design.
    Untracked,
    /// Reading failed; `message` says why.
    Unavailable,
}

/// One account's quota as the UI shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountQuota {
    /// The agent id sessions run under, or `default:<provider>` for the login
    /// the CLI uses when no account is selected.
    pub account_id: String,
    pub provider: AccountProvider,
    pub label: String,
    pub display_name: String,
    pub is_default: bool,
    pub plan: Option<String>,
    pub status: QuotaStatus,
    pub message: Option<String>,
    pub windows: Vec<QuotaWindow>,
    /// Unix seconds the figures describe.
    pub updated_at: Option<i64>,
}

/// Something to read: an account entry with its profile home, or a default
/// login (`home: None`).
#[derive(Debug, Clone)]
struct Target {
    account_id: String,
    provider: AccountProvider,
    label: String,
    display_name: String,
    home: Option<PathBuf>,
}

impl Target {
    fn quota(&self, status: QuotaStatus) -> AccountQuota {
        AccountQuota {
            account_id: self.account_id.clone(),
            provider: self.provider,
            label: self.label.clone(),
            display_name: self.display_name.clone(),
            is_default: self.home.is_none(),
            plan: None,
            status,
            message: None,
            windows: Vec::new(),
            updated_at: None,
        }
    }
}

/// Whether `program` resolves on the (already enriched) `PATH`.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            let candidate = dir.join(program);
            candidate.is_file()
        })
    })
}

/// The default logins first (only for CLIs that are installed), then every
/// account entry.
fn targets(app: &AppHandle) -> Vec<Target> {
    let mut targets = Vec::new();
    if on_path("claude") {
        targets.push(Target {
            account_id: "default:claude".into(),
            provider: AccountProvider::Claude,
            label: "default".into(),
            display_name: "Claude Code".into(),
            home: None,
        });
    }
    if on_path("codex") {
        targets.push(Target {
            account_id: "default:codex".into(),
            provider: AccountProvider::Codex,
            label: "default".into(),
            display_name: "Codex".into(),
            home: None,
        });
    }
    if let Some(host) = app.try_state::<Arc<AgentHost>>() {
        targets.extend(installed_accounts(&host).into_iter().map(|account| Target {
            display_name: account
                .display_name
                .clone()
                .unwrap_or_else(|| format!("{} · {}", account.base_agent_id, account.label)),
            account_id: account.id,
            provider: account.provider,
            label: account.label,
            home: Some(PathBuf::from(account.profile_home)),
        }));
    }
    targets
}

async fn read_target(target: Target) -> AccountQuota {
    match target.provider {
        AccountProvider::Codex => read_codex(&target).await,
        AccountProvider::Claude => read_claude(&target).await,
    }
}

async fn read_codex(target: &Target) -> AccountQuota {
    match codex::read(target.home.as_deref()).await {
        Ok(reading) => AccountQuota {
            plan: reading.plan,
            windows: reading.windows,
            updated_at: Some(now_secs()),
            ..target.quota(QuotaStatus::Ok)
        },
        Err(codex::CodexError::SignedOut) => AccountQuota {
            message: Some("Not signed in. Start a chat with this account to sign in.".into()),
            ..target.quota(QuotaStatus::SignedOut)
        },
        Err(codex::CodexError::NotInstalled) => AccountQuota {
            message: Some("Install the Codex CLI (`codex`) to read this account's limits.".into()),
            ..target.quota(QuotaStatus::Unavailable)
        },
        Err(codex::CodexError::Failed(reason)) => AccountQuota {
            message: Some(reason),
            ..target.quota(QuotaStatus::Unavailable)
        },
    }
}

async fn read_claude(target: &Target) -> AccountQuota {
    // The default login's readings come from runs through `quotatlas claude`,
    // which saves them outside `~/.claude`.
    let Some(dir) = target.home.clone().or_else(claude::default_reading_dir) else {
        return target.quota(QuotaStatus::Unavailable);
    };
    let profile = target.home.is_some();
    let reading = tokio::task::spawn_blocking(move || {
        // Accounts made before quota existed get their status line here.
        if profile {
            if let Err(e) = claude::install_statusline(&dir) {
                tracing::warn!(target: "quotatlas::quota", "status line for {}: {e}", dir.display());
            }
        }
        claude::read(&dir)
    })
    .await
    .ok()
    .flatten();
    match reading {
        Some(reading) if !reading.windows.is_empty() => AccountQuota {
            windows: reading.windows,
            updated_at: reading.updated_at,
            ..target.quota(QuotaStatus::Ok)
        },
        _ if !profile => AccountQuota {
            message: Some(
                "Your default Claude Code login is read without editing ~/.claude: run it once \
                 through `quotatlas claude` in a terminal to load its limits."
                    .into(),
            ),
            ..target.quota(QuotaStatus::Untracked)
        },
        _ => AccountQuota {
            message: Some(
                "Claude Code reports limits through its status line. Use this account once in a \
                 terminal to load them."
                    .into(),
            ),
            ..target.quota(QuotaStatus::Pending)
        },
    }
}

/// Holds the last snapshot and keeps refreshes from overlapping.
#[derive(Default)]
pub struct QuotaService {
    snapshot: parking_lot::Mutex<Vec<AccountQuota>>,
    refreshing: AtomicBool,
}

/// Clears the in-flight flag however a refresh ends.
struct RefreshInFlight<'a>(&'a AtomicBool);

impl Drop for RefreshInFlight<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl QuotaService {
    pub fn snapshot(&self) -> Vec<AccountQuota> {
        self.snapshot.lock().clone()
    }

    /// Read every account now, store the result and broadcast it. A refresh
    /// requested while one is running returns the last snapshot instead of
    /// doubling the CLI calls; the running one broadcasts when it finishes.
    pub async fn refresh(&self, app: &AppHandle) -> Vec<AccountQuota> {
        if self
            .refreshing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return self.snapshot();
        }
        let _in_flight = RefreshInFlight(&self.refreshing);
        let mut reads = tokio::task::JoinSet::new();
        for (index, target) in targets(app).into_iter().enumerate() {
            reads.spawn(async move { (index, read_target(target).await) });
        }
        let mut results = Vec::new();
        while let Some(joined) = reads.join_next().await {
            if let Ok(result) = joined {
                results.push(result);
            }
        }
        results.sort_by_key(|(index, _)| *index);
        let quotas: Vec<AccountQuota> = results.into_iter().map(|(_, quota)| quota).collect();
        *self.snapshot.lock() = quotas.clone();
        let _ = app.emit(QUOTA_CHANGED_EVENT, &quotas);
        quotas
    }
}

/// Refresh shortly after launch, then every `POLL_INTERVAL`, for the life of
/// the app.
pub fn start_polling(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_POLL_DELAY).await;
        loop {
            if let Some(service) = app.try_state::<Arc<QuotaService>>() {
                let service = service.inner().clone();
                service.refresh(&app).await;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}
