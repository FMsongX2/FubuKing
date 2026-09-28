//! Codex quota, read through the Codex CLI's own app server (ADR Q-0002).
//!
//! FubuKing starts `codex app-server --stdio` with the account's
//! `CODEX_HOME` and asks it over line-delimited JSON-RPC for `account/read`
//! (the plan) and `account/rateLimits/read` (the windows). The CLI answers
//! from its own login; FubuKing only ever sees the answers. The request
//! sequence is the app-server protocol, the same one Quotio's Codex provider
//! (MIT) uses.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use super::QuotaWindow;
use crate::accounts::Account;

/// Upper bound for one whole read: spawn, handshake and both requests.
const READ_TIMEOUT: Duration = Duration::from_secs(20);
/// Lines skipped while waiting for one reply; notifications are allowed, a
/// flood is not.
const MAX_LINES_PER_REPLY: usize = 256;
/// The bucket Codex's own limits live in; other buckets are listed after it.
const MAIN_BUCKET: &str = "codex";

/// What one successful read returns.
#[derive(Debug, Clone, PartialEq)]
pub struct CodexReading {
    pub plan: Option<String>,
    pub windows: Vec<QuotaWindow>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodexError {
    /// No `codex` executable on the enriched `PATH`.
    NotInstalled,
    /// The CLI has no login for this home.
    SignedOut,
    Failed(String),
}

/// Read one account's plan and windows. `profile_home` is `None` for the
/// default login in `~/.codex`.
pub async fn read(profile_home: Option<&Path>) -> Result<CodexReading, CodexError> {
    let mut cmd = atlas_process::async_command(crate::executable("codex"));
    cmd.args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    match profile_home {
        Some(home) => cmd.env("CODEX_HOME", home),
        None => cmd.env_remove("CODEX_HOME"),
    };
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => CodexError::NotInstalled,
        _ => CodexError::Failed(format!("could not start the Codex CLI: {e}")),
    })?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err(CodexError::Failed("the Codex CLI has no stdio".into()));
    };
    let outcome = tokio::time::timeout(READ_TIMEOUT, session(BufReader::new(stdout), stdin)).await;
    let _ = child.kill().await;
    outcome.map_err(|_| CodexError::Failed("the Codex CLI did not answer in time".into()))?
}

/// The protocol exchange, over any line-oriented stream pair so tests can
/// drive it without a process.
async fn session<R, W>(mut reader: R, mut writer: W) -> Result<CodexReading, CodexError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    call(
        &mut reader,
        &mut writer,
        1,
        "initialize",
        json!({
            "clientInfo": { "name": "fubuking", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "experimentalApi": false },
        }),
    )
    .await?;
    send(&mut writer, &json!({ "method": "initialized" })).await?;
    let account = call(&mut reader, &mut writer, 2, "account/read", json!({ "refreshToken": false })).await?;
    let plan = plan_from_account(&account)?;
    let limits = call(&mut reader, &mut writer, 3, "account/rateLimits/read", json!({})).await?;
    Ok(CodexReading { plan, windows: windows_from_limits(&limits) })
}

async fn send<W: AsyncWrite + Unpin>(writer: &mut W, message: &Value) -> Result<(), CodexError> {
    let mut line = message.to_string();
    line.push('\n');
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|e| CodexError::Failed(format!("writing to the Codex CLI: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| CodexError::Failed(format!("writing to the Codex CLI: {e}")))
}

/// One request, and the reply carrying its id. Anything else on the stream
/// (notifications, unrelated replies) is skipped.
async fn call<R, W>(
    reader: &mut R,
    writer: &mut W,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, CodexError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    send(writer, &json!({ "id": id, "method": method, "params": params })).await?;
    let mut line = String::new();
    for _ in 0..MAX_LINES_PER_REPLY {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|e| CodexError::Failed(format!("reading from the Codex CLI: {e}")))?;
        if read == 0 {
            return Err(CodexError::Failed("the Codex CLI exited before answering".into()));
        }
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if message.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            let text = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("request failed")
                .to_string();
            return Err(if mentions_login(&text) {
                CodexError::SignedOut
            } else {
                CodexError::Failed(text)
            });
        }
        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
    }
    Err(CodexError::Failed("the Codex CLI never answered".into()))
}

fn mentions_login(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    ["not logged in", "log in", "login", "sign in", "unauthorized", "auth"]
        .iter()
        .any(|needle| text.contains(needle))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAccount {
    #[serde(rename = "type")]
    kind: Option<String>,
    plan_type: Option<String>,
}

/// The plan, from `account/read`. No account means no login. Only the plan
/// is kept: the reply also carries the account's email, which FubuKing has no
/// use for and drops.
fn plan_from_account(result: &Value) -> Result<Option<String>, CodexError> {
    let account = result.get("account").filter(|a| !a.is_null()).ok_or(CodexError::SignedOut)?;
    let account: RawAccount = serde_json::from_value(account.clone())
        .map_err(|e| CodexError::Failed(format!("unexpected account reply: {e}")))?;
    Ok(match account.kind.as_deref() {
        Some("chatgpt") => account.plan_type.filter(|plan| !plan.trim().is_empty()),
        Some("apiKey") => Some("API key".to_string()),
        _ => account.plan_type,
    })
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawWindow {
    used_percent: Option<f64>,
    window_duration_mins: Option<u64>,
    resets_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBucket {
    limit_id: Option<String>,
    limit_name: Option<String>,
    primary: Option<RawWindow>,
    secondary: Option<RawWindow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLimits {
    rate_limits: Option<RawBucket>,
    rate_limits_by_limit_id: Option<BTreeMap<String, RawBucket>>,
}

/// The windows in `account/rateLimits/read`, the main bucket first.
///
/// A window is named by its length (five hours is the session, a week is the
/// weekly limit) because the CLI does not say which slot holds which: today
/// the primary slot can be the weekly one. Windows without a usage figure are
/// skipped; the CLI sends `{}` for an empty slot.
fn windows_from_limits(result: &Value) -> Vec<QuotaWindow> {
    let Ok(limits) = serde_json::from_value::<RawLimits>(result.clone()) else {
        return Vec::new();
    };
    let mut buckets: Vec<(String, RawBucket)> = match limits.rate_limits_by_limit_id {
        Some(map) if !map.is_empty() => map.into_iter().collect(),
        _ => limits
            .rate_limits
            .map(|bucket| {
                let id = bucket.limit_id.clone().unwrap_or_else(|| MAIN_BUCKET.to_string());
                vec![(id, bucket)]
            })
            .unwrap_or_default(),
    };
    buckets.sort_by_key(|(id, _)| (id != MAIN_BUCKET, id.clone()));

    let mut windows = Vec::new();
    for (id, bucket) in buckets {
        let prefix = match bucket.limit_name.as_deref().filter(|name| !name.is_empty()) {
            _ if id == MAIN_BUCKET => String::new(),
            Some(name) => format!("{name} "),
            None => format!("{id} "),
        };
        let mut slots: Vec<RawWindow> = [bucket.primary, bucket.secondary]
            .into_iter()
            .flatten()
            .filter(|window| window.used_percent.is_some())
            .collect();
        slots.sort_by_key(|window| window.window_duration_mins.unwrap_or(u64::MAX));
        for (index, window) in slots.into_iter().enumerate() {
            let minutes = window.window_duration_mins;
            windows.push(QuotaWindow {
                id: format!("{id}-{}", window_slug(minutes, index)),
                label: format!("{prefix}{}", window_label(minutes)),
                used_percent: window.used_percent.unwrap_or(0.0).clamp(0.0, 100.0),
                window_minutes: minutes,
                resets_at: window.resets_at,
            });
        }
    }
    windows
}

fn window_label(minutes: Option<u64>) -> String {
    match minutes {
        Some(300) => "Session".to_string(),
        Some(10_080) => "Weekly".to_string(),
        Some(m) if m % 1_440 == 0 => format!("{}-day", m / 1_440),
        Some(m) if m % 60 == 0 => format!("{}-hour", m / 60),
        Some(m) => format!("{m}-minute"),
        None => "Limit".to_string(),
    }
}

fn window_slug(minutes: Option<u64>, index: usize) -> String {
    match minutes {
        Some(300) => "session".to_string(),
        Some(10_080) => "weekly".to_string(),
        Some(m) => format!("{m}m"),
        None => format!("slot-{index}"),
    }
}

/// The answer that carries on when the Codex CLI asks whether to trust `cwd`
/// under `account`, or `None` when it will not ask or its config cannot be
/// read. A folder never answered offers "Trust and continue"; one marked
/// untrusted gets the restricted screen, where "Open restricted" carries on.
pub fn trust_answer(account: &Account, cwd: &Path) -> Option<&'static str> {
    match trust_level(account, cwd)?.as_deref() {
        Some("trusted") => None,
        Some("untrusted") => Some("Open restricted"),
        _ => Some("Trust and continue"),
    }
}

/// The `trust_level` Codex starts `cwd` with under `account`, looked up as
/// Codex 0.157.1 does it: the folder's entry in `[projects]` of the account's
/// `config.toml`, else its repository's; the first entry found decides, with
/// or without a level. `Some(None)` for no level, `None` when the config
/// cannot be read.
fn trust_level(account: &Account, cwd: &Path) -> Option<Option<String>> {
    let config: toml::Table = match std::fs::read_to_string(account.cli_home()?.join("config.toml")) {
        Ok(raw) => raw.parse().ok()?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
    };
    let Some(projects) = config.get("projects").and_then(toml::Value::as_table) else {
        return Some(None);
    };
    let level = [Some(cwd.to_path_buf()), repo_root(cwd)]
        .into_iter()
        .flatten()
        .flat_map(|dir| [dunce::canonicalize(&dir).ok(), Some(dir)])
        .flatten()
        .find_map(|dir| {
            let key = dir.to_string_lossy();
            let (_, project) = projects.iter().find(|(name, _)| same_key(name, &key))?;
            Some(project.get("trust_level").and_then(toml::Value::as_str).map(str::to_string))
        });
    Some(level.flatten())
}

/// The repository whose answer covers `cwd`, as Codex finds it: the nearest
/// checkout above it, or for a linked worktree the main checkout. `None` for
/// a submodule or any other `.git` file that does not point into
/// `<main>/.git/worktrees/`.
fn repo_root(cwd: &Path) -> Option<PathBuf> {
    let checkout = cwd.ancestors().find(|dir| {
        let git = dir.join(".git");
        git.is_file() || git.join("HEAD").exists()
    })?;
    let git = checkout.join(".git");
    if git.is_dir() {
        return Some(checkout.to_path_buf());
    }
    let pointer = std::fs::read_to_string(&git).ok()?;
    let git_dir = checkout.join(pointer.strip_prefix("gitdir:")?.trim());
    let worktrees = git_dir.parent()?;
    if worktrees.file_name()? != "worktrees" {
        return None;
    }
    worktrees.parent()?.parent().map(Path::to_path_buf)
}

/// Codex compares project keys without case on Windows.
fn same_key(stored: &str, key: &str) -> bool {
    if cfg!(windows) {
        stored.eq_ignore_ascii_case(key)
    } else {
        stored == key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `codex-cli 0.147.0` returned for a Plus account on
    /// 2026-09-28: the weekly window in the primary slot, an empty secondary.
    fn live_limits() -> Value {
        json!({
            "rateLimitResetCredits": null,
            "rateLimits": {
                "limitId": "codex", "limitName": null,
                "primary": { "usedPercent": 11, "windowDurationMins": 10080, "resetsAt": 1791099707 },
                "secondary": {}
            },
            "rateLimitsByLimitId": {
                "base_model_inference": {
                    "limitId": "base_model_inference", "limitName": "gpt-reserve",
                    "primary": { "usedPercent": 0, "windowDurationMins": 10080, "resetsAt": 1791128165 },
                    "secondary": {}
                },
                "codex": {
                    "limitId": "codex", "limitName": null,
                    "primary": { "usedPercent": 11, "windowDurationMins": 10080, "resetsAt": 1791099707 },
                    "secondary": {}
                }
            }
        })
    }

    #[test]
    fn windows_are_named_by_length_with_the_main_bucket_first() {
        let windows = windows_from_limits(&live_limits());
        let labels: Vec<&str> = windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, ["Weekly", "gpt-reserve Weekly"]);
        assert_eq!(windows[0].id, "codex-weekly");
        assert_eq!(windows[0].used_percent, 11.0);
        assert_eq!(windows[0].resets_at, Some(1791099707));
        assert_eq!(windows[0].window_minutes, Some(10_080));
    }

    #[test]
    fn a_session_window_sorts_before_the_weekly_one_whatever_its_slot() {
        let limits = json!({ "rateLimits": {
            "limitId": "codex",
            "primary": { "usedPercent": 29, "windowDurationMins": 10080, "resetsAt": 2 },
            "secondary": { "usedPercent": 9, "windowDurationMins": 300, "resetsAt": 1 }
        }});
        let labels: Vec<String> = windows_from_limits(&limits).into_iter().map(|w| w.label).collect();
        assert_eq!(labels, ["Session", "Weekly"]);
    }

    #[test]
    fn unknown_lengths_get_a_readable_label() {
        assert_eq!(window_label(Some(1_440)), "1-day");
        assert_eq!(window_label(Some(120)), "2-hour");
        assert_eq!(window_label(Some(45)), "45-minute");
        assert_eq!(window_label(None), "Limit");
    }

    #[test]
    fn the_plan_is_kept_and_a_missing_account_means_signed_out() {
        let plus = json!({ "account": { "type": "chatgpt", "email": "a@b.c", "planType": "plus" } });
        assert_eq!(plan_from_account(&plus), Ok(Some("plus".to_string())));
        assert_eq!(plan_from_account(&json!({ "account": null })), Err(CodexError::SignedOut));
        let key = json!({ "account": { "type": "apiKey" } });
        assert_eq!(plan_from_account(&key), Ok(Some("API key".to_string())));
    }

    /// A scripted app server on an in-memory pipe: it interleaves a
    /// notification before each reply, the way the real one may.
    async fn run_session(replies: Vec<Value>) -> Result<CodexReading, CodexError> {
        let (client_side, mut server_side) = tokio::io::duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client_side);
        let server = tokio::spawn(async move {
            let (server_read, mut server_write) = tokio::io::split(&mut server_side);
            let mut lines = BufReader::new(server_read).lines();
            let mut replies = replies.into_iter();
            while let Ok(Some(line)) = lines.next_line().await {
                let request: Value = serde_json::from_str(&line).unwrap();
                let Some(id) = request.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                let Some(mut reply) = replies.next() else { break };
                reply["id"] = json!(id);
                let notice = json!({ "method": "thread/started", "params": {} });
                let out = format!("{notice}\n{reply}\n");
                server_write.write_all(out.as_bytes()).await.unwrap();
            }
        });
        let result = session(BufReader::new(client_read), client_write).await;
        server.abort();
        result
    }

    #[tokio::test]
    async fn a_full_session_returns_the_plan_and_windows() {
        let reading = run_session(vec![
            json!({ "result": {} }),
            json!({ "result": { "account": { "type": "chatgpt", "planType": "pro" } } }),
            json!({ "result": live_limits() }),
        ])
        .await
        .unwrap();
        assert_eq!(reading.plan.as_deref(), Some("pro"));
        assert_eq!(reading.windows.len(), 2);
    }

    /// Against the real Codex CLI and its default login. Ignored by default:
    /// it needs `codex` on PATH and a signed-in account. Run with
    /// `cargo test -p fubuking --lib quota::codex -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "needs a signed-in Codex CLI"]
    async fn reads_the_default_login_live() {
        let reading = read(None).await.expect("the default Codex login answers");
        println!("plan: {:?}", reading.plan);
        for window in &reading.windows {
            println!("{}: {}% used, resets {:?}", window.label, window.used_percent, window.resets_at);
        }
        assert!(!reading.windows.is_empty());
    }

    fn codex_account(home: &Path) -> Account {
        Account {
            id: "codex-acp-p".into(),
            provider: crate::accounts::Provider::Codex,
            label: "p".into(),
            home: Some(home.to_path_buf()),
        }
    }

    fn trust(home: &Path, entries: &[(&Path, &str)]) {
        let body: String = entries
            .iter()
            .map(|(dir, level)| format!("[projects.{:?}]\ntrust_level = \"{level}\"\n", dunce::canonicalize(dir).unwrap()))
            .collect();
        std::fs::write(home.join("config.toml"), body).unwrap();
    }

    #[test]
    fn the_answer_follows_the_accounts_own_config_and_a_new_home_has_none() {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let account = codex_account(home.path());
        assert_eq!(trust_answer(&account, folder.path()), Some("Trust and continue"));
        trust(home.path(), &[(folder.path(), "trusted")]);
        assert_eq!(trust_answer(&account, folder.path()), None);
        trust(home.path(), &[(folder.path(), "untrusted")]);
        assert_eq!(trust_answer(&account, folder.path()), Some("Open restricted"));
        std::fs::write(home.path().join("config.toml"), "not = [toml").unwrap();
        assert_eq!(trust_answer(&account, folder.path()), None);
    }

    #[test]
    fn a_trusted_repository_covers_its_subfolders_and_linked_worktrees_but_not_submodules() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("main");
        std::fs::create_dir_all(main.join(".git/worktrees/side")).unwrap();
        std::fs::create_dir_all(main.join(".git/modules/lib")).unwrap();
        std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir_all(main.join("src/deep")).unwrap();
        let side = root.path().join("side");
        std::fs::create_dir_all(&side).unwrap();
        std::fs::write(side.join(".git"), format!("gitdir: {}\n", main.join(".git/worktrees/side").display())).unwrap();
        let lib = main.join("lib");
        std::fs::create_dir_all(lib.join("src")).unwrap();
        std::fs::write(lib.join(".git"), "gitdir: ../.git/modules/lib\n").unwrap();
        let account = codex_account(home.path());

        trust(home.path(), &[(&main, "trusted")]);
        assert_eq!(trust_answer(&account, &main.join("src/deep")), None);
        assert_eq!(trust_answer(&account, &side), None);
        // A submodule is its own project to Codex.
        trust(home.path(), &[(&lib, "trusted")]);
        assert_eq!(trust_answer(&account, &lib), None);
        assert_eq!(trust_answer(&account, &lib.join("src")), Some("Trust and continue"));

        // The folder's own answer comes first.
        trust(home.path(), &[(&main.join("src"), "untrusted"), (&main, "trusted")]);
        assert_eq!(trust_answer(&account, &main.join("src")), Some("Open restricted"));
    }

    #[tokio::test]
    async fn an_auth_error_reads_as_signed_out() {
        let outcome = run_session(vec![
            json!({ "result": {} }),
            json!({ "error": { "code": -32000, "message": "Not logged in" } }),
        ])
        .await;
        assert_eq!(outcome, Err(CodexError::SignedOut));
    }
}
