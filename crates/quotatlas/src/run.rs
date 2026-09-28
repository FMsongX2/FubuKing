//! `quotatlas claude` and `quotatlas codex`: run the agent CLI under the
//! account with the most room, with the shared-memory server attached, and
//! when a session stops on its usage limit, resume that same session on the
//! next account instead of starting over.

use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::ExitStatus;
use std::time::SystemTime;

use anyhow::Context;

use crate::accounts::{self, Account, Provider};
use crate::mcp;
use crate::quota::{self, claude};
use crate::sessions;

/// The first message of a resumed session.
pub const CONTINUE_PROMPT: &str = "The previous account hit its usage limit, so this session moved to \
     another account. Continue from where you stopped.";

pub struct Run {
    pub provider: Provider,
    /// An account label or id; the account with the most room when `None`.
    pub account: Option<String>,
    /// Passed to the CLI as given on the first launch. A resumed session
    /// starts from the resume arguments alone.
    pub args: Vec<OsString>,
}

/// Run the CLI until it exits other than on a usage limit, or no account is
/// left to continue on. Returns the CLI's exit code.
pub async fn run(run: Run) -> anyhow::Result<i32> {
    let cwd = std::env::current_dir().context("reading the current directory")?;
    let program = run.provider.program();
    let accounts = accounts::list(run.provider);
    let mut account = match &run.account {
        Some(wanted) => accounts::find(&accounts, wanted)
            .cloned()
            .with_context(|| format!("no {program} account `{wanted}`; `quotatlas quota` lists them"))?,
        None => pick(&accounts, &[]).await.unwrap_or_else(|| Account::default_for(run.provider)),
    };
    let _interrupts = survive_interrupts();
    let mut args = run.args;
    let mut tried = Vec::new();
    loop {
        tried.push(account.id.clone());
        let started = SystemTime::now();
        let code = exit_code(launch(&account, &cwd, &args).await?);
        let Some(session) = sessions::latest(&account, &cwd, started) else { return Ok(code) };
        let Some(message) = sessions::limit_message(&session) else { return Ok(code) };
        eprintln!("quotatlas: {account} stopped on its usage limit: {message}");
        let Some(next) = pick(&accounts, &tried).await else {
            eprintln!("quotatlas: no other {program} account has room. Add one with `quotatlas login {program} <name>`.");
            return Ok(code);
        };
        if !confirm(&format!("quotatlas: resume this session on {next}? [Y/n] ")) {
            return Ok(code);
        }
        sessions::copy_to(&session, &next)
            .with_context(|| format!("copying session {} to {next}", session.id))?;
        args = sessions::resume_args(&session, CONTINUE_PROMPT).into_iter().map(OsString::from).collect();
        account = next;
    }
}

/// Create the account `label` and sign it in: Claude Code signs in from its
/// own session, Codex with `codex login`.
pub async fn login(provider: Provider, label: &str) -> anyhow::Result<i32> {
    let account = accounts::create(provider, label).context("creating the account")?;
    let cwd = std::env::current_dir().context("reading the current directory")?;
    let args: Vec<OsString> = match provider {
        Provider::Claude => {
            eprintln!("quotatlas: sign in with /login in the Claude Code session that opens now, then exit it.");
            Vec::new()
        }
        Provider::Codex => vec!["login".into()],
    };
    let code = exit_code(launch(&account, &cwd, &args).await?);
    if code == 0 {
        eprintln!("quotatlas: {account} is ready: `quotatlas {} --account {}`", provider.program(), account.label);
    }
    Ok(code)
}

/// The untried account with the most room. Accounts whose room is unknown
/// come after the ones known to have room, in list order; accounts known to
/// be full are skipped.
async fn pick(accounts: &[Account], tried: &[String]) -> Option<Account> {
    let candidates: Vec<Account> = accounts.iter().filter(|a| !tried.contains(&a.id)).cloned().collect();
    // A first run with a single account has nothing to choose between, and
    // reading Codex's room would only delay the start.
    if tried.is_empty() && candidates.len() == 1 {
        return candidates.into_iter().next();
    }
    let now = quota::now_secs();
    let mut reads = tokio::task::JoinSet::new();
    for (index, account) in candidates.iter().cloned().enumerate() {
        reads.spawn(async move { (index, quota::headroom_of(&account, now).await) });
    }
    let mut rooms = vec![None; candidates.len()];
    while let Some(joined) = reads.join_next().await {
        if let Ok((index, room)) = joined {
            rooms[index] = room;
        }
    }
    let rank = |room: &Option<f64>| room.unwrap_or(-1.0);
    candidates
        .into_iter()
        .zip(rooms)
        .filter(|(_, room)| room.is_none_or(|left| left > 0.0))
        // `min_by` keeps the first of equals, which keeps list order.
        .min_by(|(_, a), (_, b)| rank(b).total_cmp(&rank(a)))
        .map(|(account, _)| account)
}

async fn launch(account: &Account, cwd: &Path, args: &[OsString]) -> anyhow::Result<ExitStatus> {
    let program = account.provider.program();
    let (before, after) = added_args(account, cwd);
    tokio::process::Command::new(program)
        .args(before)
        .args(args)
        .args(after)
        .envs(account.env())
        .status()
        .await
        .with_context(|| format!("starting `{program}`; is it installed and on PATH?"))
}

/// What Quotatlas adds around the user's arguments, as (before, after): the
/// shared-memory server for this run, and for Claude the status line quota is
/// read through. Nothing is written to the CLI's own configuration.
///
/// Codex takes `-c` overrides ahead of any subcommand. Claude's
/// `--mcp-config` takes every value up to the next option, so it goes last,
/// where it cannot swallow a prompt.
fn added_args(account: &Account, cwd: &Path) -> (Vec<OsString>, Vec<OsString>) {
    match account.provider {
        Provider::Codex => {
            let (command, args) = mcp::server_command("codex");
            let toml = |value: serde_json::Value| value.to_string();
            let key = format!("mcp_servers.{}", mcp::SERVER_NAME);
            // Memory tools run without a prompt, as the desktop app runs them:
            // they only read and write the project's own record.
            let before = [
                format!("{key}.command={}", toml(command.into())),
                format!("{key}.args={}", toml(args.into())),
                format!("{key}.default_tools_approval_mode=\"approve\""),
            ];
            (before.into_iter().flat_map(|value| ["-c".to_string(), value]).map(OsString::from).collect(), Vec::new())
        }
        Provider::Claude => {
            let (command, args) = mcp::server_command("claude-code");
            let config = serde_json::json!({
                "mcpServers": { mcp::SERVER_NAME: { "type": "stdio", "command": command, "args": args } }
            });
            let mut after: Vec<OsString> = status_line_args(account, cwd);
            after.extend([
                "--allowedTools".into(),
                format!("mcp__{}", mcp::SERVER_NAME).into(),
                "--mcp-config".into(),
                config.to_string().into(),
            ]);
            (Vec::new(), after)
        }
    }
}

/// Claude needs its status line for quota: a profile has it in its own
/// settings, the default login gets it per run unless the user has a status
/// line of their own.
fn status_line_args(account: &Account, cwd: &Path) -> Vec<OsString> {
    if let Some(home) = &account.home {
        if let Err(e) = claude::install_statusline(home) {
            eprintln!("quotatlas: no quota for {account}: {e}");
        }
        return Vec::new();
    }
    let settings = [
        account.cli_home().map(|home| home.join("settings.json")),
        Some(cwd.join(".claude/settings.json")),
        Some(cwd.join(".claude/settings.local.json")),
    ];
    // ponytail: a status line the user set means no quota for the default
    // login; chaining theirs after ours would lift that.
    if settings.iter().flatten().any(|file| claude::sets_status_line(file)) {
        return Vec::new();
    }
    match claude::default_reading_dir().map(|dir| claude::write_script(&dir)) {
        Some(Ok(script)) => vec!["--settings".into(), claude::status_line_settings(&script).to_string().into()],
        _ => Vec::new(),
    }
}

/// Ask on the terminal; Enter means yes. Never asks, and says no, when
/// nobody is at a terminal: a script's run must not turn into a session.
fn confirm(question: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprint!("{question}");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).unwrap_or(0) == 0 {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "" | "y" | "yes")
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

/// Keep this process alive through Ctrl-C and Ctrl-\ while the CLI runs:
/// they are the CLI's to handle. A handler rather than an ignored signal, so
/// the CLI still starts with the default disposition.
#[cfg(unix)]
fn survive_interrupts() -> Vec<tokio::signal::unix::Signal> {
    use tokio::signal::unix::{signal, SignalKind};
    [SignalKind::interrupt(), SignalKind::quit()]
        .into_iter()
        .filter_map(|kind| signal(kind).ok())
        .collect()
}

#[cfg(windows)]
fn survive_interrupts() -> Option<tokio::signal::windows::CtrlC> {
    tokio::signal::windows::ctrl_c().ok()
}
