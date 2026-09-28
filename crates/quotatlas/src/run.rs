//! `quotatlas claude` and `quotatlas codex`: run the agent CLI under the
//! account with the most room, with the shared-memory server attached, and
//! when a session stops on its usage limit, carry on instead of starting over:
//! the same session on the next account of that CLI, or, when none has room,
//! a brief of it in the other CLI.
//!
//! An interactive CLI that hits an account's limit is stopped as soon as the
//! transcript shows it, so the handoff does not wait for the user to exit.

use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::{Duration, SystemTime};

use anyhow::Context;

use crate::accounts::{self, Account, Provider};
use crate::args::{self, Options};
use crate::mcp;
use crate::quota::{self, claude};
use crate::sessions::{self, Session};

/// The first message of a resumed session.
pub const CONTINUE_PROMPT: &str = "The previous account hit its usage limit, so this session moved to \
     another account. Continue from where you stopped.";

/// How often a running CLI's transcript is checked for a limit hit.
const WATCH_INTERVAL: Duration = Duration::from_secs(2);
/// How long a stopped CLI gets to exit after each signal.
const STOP_GRACE: Duration = Duration::from_secs(3);

pub struct Run {
    pub provider: Provider,
    /// An account label or id; the account with the most room when `None`.
    pub account: Option<String>,
    /// Passed to the CLI as given on the first launch.
    pub args: Vec<OsString>,
}

/// How the first run was driven. A handoff keeps it: an interactive session,
/// or one prompt run to completion (`claude -p`, `codex exec`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Interactive,
    Batch,
}

/// Run the CLI until it exits other than on a usage limit, or nothing is left
/// to carry on with. Returns the CLI's exit code.
pub async fn run(run: Run) -> anyhow::Result<i32> {
    let cwd = std::env::current_dir().context("reading the current directory")?;
    let mut provider = run.provider;
    let mut accounts = accounts::list(provider);
    let mut account = match &run.account {
        Some(wanted) => accounts::find(&accounts, wanted)
            .cloned()
            .with_context(|| format!("no {} account `{wanted}`; `quotatlas quota` lists them", provider.program()))?,
        None => pick(accounts.clone(), true).await.unwrap_or_else(|| Account::default_for(provider)),
    };
    let first = FirstRun::new(run.provider, run.args.clone());
    let _interrupts = survive_interrupts();
    let mut launch_args = run.args;
    let mut tried: Vec<(Provider, String)> = Vec::new();
    // The session a resume launch is meant to continue, and where its copy went.
    let mut resuming: Option<(Session, PathBuf)> = None;
    loop {
        tried.push((provider, account.id.clone()));
        let started = SystemTime::now();
        let watch = std::io::stdin().is_terminal() && first.mode() == Mode::Interactive;
        let code = exit_code(launch(&account, &cwd, &launch_args, watch.then_some(started)).await?);
        let latest = sessions::latest(&account, &cwd, started);

        if let Some((previous, copy)) = resuming.take() {
            // A resume that failed and never touched the copy means the CLI
            // did not find it: its layout changed. Start the account over
            // from a brief of the session instead.
            let touched = std::fs::metadata(&copy).and_then(|meta| meta.modified()).is_ok_and(|at| at >= started);
            // Asked, not assumed: the same exit is what declining Claude's
            // folder-trust question looks like.
            if code != 0
                && !touched
                && confirm(&format!(
                    "quotatlas: {account} did not resume session {}. Start a new session there from a brief of it? [Y/n] ",
                    previous.id
                ))
            {
                let brief = sessions::brief(&previous, &account).context("reading the session for a brief")?;
                launch_args = fresh_args(provider, first.mode(), &first.options_for(provider), &brief);
                continue;
            }
        }

        let Some(session) = latest else { return Ok(code) };
        let Some(message) = sessions::limit_message(&session, started) else { return Ok(code) };
        eprintln!("quotatlas: {account} stopped on its usage limit: {message}");

        let untried = |list: &[Account], of: Provider| -> Vec<Account> {
            list.iter().filter(|a| !tried.contains(&(of, a.id.clone()))).cloned().collect()
        };
        if let Some(next) = pick(untried(&accounts, provider), false).await {
            if !confirm(&format!("quotatlas: resume this session on {next}? [Y/n] ")) {
                return Ok(code);
            }
            let copy = sessions::copy_to(&session, &next)
                .with_context(|| format!("copying session {} to {next}", session.id))?;
            if provider == Provider::Claude && claude::trusts_folder(&next, &cwd) == Some(false) {
                eprintln!("quotatlas: {next} has not trusted this folder yet. When Claude asks, choose \"Yes, I trust this folder\".");
            }
            launch_args = resume_args(provider, first.mode(), &first.options_for(provider), &session.id);
            account = next;
            resuming = Some((session, copy));
            continue;
        }

        let other = other_provider(provider);
        let others = if on_path(other.program()) { untried(&accounts::list(other), other) } else { Vec::new() };
        if let Some(next) = pick(others, false).await {
            let agent = if other == Provider::Codex { "Codex" } else { "Claude Code" };
            if !confirm(&format!(
                "quotatlas: no other {} account has room. Continue in {agent} on {next} with a brief of this session? [Y/n] ",
                provider.program()
            )) {
                return Ok(code);
            }
            let brief = sessions::brief(&session, &account).context("reading the session for a brief")?;
            launch_args = fresh_args(other, first.mode(), &first.options_for(other), &brief);
            provider = other;
            accounts = accounts::list(other);
            account = next;
            continue;
        }

        eprintln!(
            "quotatlas: no other account has room. Add one with `quotatlas login {} <name>`.",
            provider.program()
        );
        return Ok(code);
    }
}

/// What the first run was given. How it was driven and which of its words
/// are options is worked out from the CLI's help, once, when first needed.
struct FirstRun {
    provider: Provider,
    args: Vec<OsString>,
    mode: std::cell::OnceCell<Mode>,
    options: std::cell::OnceCell<Vec<OsString>>,
}

impl FirstRun {
    fn new(provider: Provider, args: Vec<OsString>) -> Self {
        Self { provider, args, mode: std::cell::OnceCell::new(), options: std::cell::OnceCell::new() }
    }

    /// Batch for `claude -p` and `codex exec`: the first word no option
    /// claims is Codex's subcommand, which only its help can tell apart from
    /// an option's value.
    fn mode(&self) -> Mode {
        *self.mode.get_or_init(|| {
            let words = self.args.iter().take_while(|w| *w != "--");
            let batch = match self.provider {
                Provider::Claude => words.clone().any(|w| w == "-p" || w == "--print"),
                Provider::Codex => {
                    let table = Options::of(self.provider.program(), None);
                    args::first_word(&self.args, &table).is_some_and(|word| word == "exec" || word == "e")
                }
            };
            if batch {
                Mode::Batch
            } else {
                Mode::Interactive
            }
        })
    }

    /// The first run's options to pass `provider`'s CLI again: all of them
    /// but the ones that pick a session, for the same CLI; none for the other.
    fn options_for(&self, provider: Provider) -> Vec<OsString> {
        if provider != self.provider {
            return Vec::new();
        }
        self.options
            .get_or_init(|| {
                let program = provider.program();
                // Codex's options that take many values attach files to one
                // prompt (`-i`), which the resumed session must not repeat;
                // they would also swallow the subcommand that follows them.
                let (table, drop, drop_many): (Options, &[&str], bool) = match provider {
                    Provider::Claude => (
                        Options::of(program, None),
                        &["-r", "--resume", "-c", "--continue", "--session-id", "--fork-session", "--from-pr"],
                        false,
                    ),
                    Provider::Codex => (
                        Options::of(program, None).merged(Options::of(program, Some("exec"))),
                        &["--last", "--all"],
                        true,
                    ),
                };
                args::options_only(&self.args, &table, drop, drop_many)
            })
            .clone()
    }
}

/// Resume `id` in the same mode, with the first run's options. Claude's
/// options can take every following word, so its prompt comes after `--`.
fn resume_args(provider: Provider, mode: Mode, options: &[OsString], id: &str) -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::new();
    match provider {
        Provider::Claude => {
            out.extend_from_slice(options);
            out.extend(["--resume".into(), id.into(), "--".into(), CONTINUE_PROMPT.into()]);
        }
        Provider::Codex => {
            if mode == Mode::Batch {
                out.push("exec".into());
            }
            out.extend_from_slice(options);
            out.extend(["resume".into(), id.into(), CONTINUE_PROMPT.into()]);
        }
    }
    out
}

/// Start a new session in the same mode with `brief` as its first message.
fn fresh_args(provider: Provider, mode: Mode, options: &[OsString], brief: &str) -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::new();
    match (provider, mode) {
        (Provider::Claude, _) => {
            out.extend_from_slice(options);
            if mode == Mode::Batch && !options.iter().any(|w| w == "-p" || w == "--print") {
                out.push("-p".into());
            }
            out.push("--".into());
        }
        (Provider::Codex, Mode::Batch) => {
            out.push("exec".into());
            out.extend_from_slice(options);
        }
        (Provider::Codex, Mode::Interactive) => out.extend_from_slice(options),
    }
    out.push(brief.into());
    out
}

fn other_provider(provider: Provider) -> Provider {
    match provider {
        Provider::Claude => Provider::Codex,
        Provider::Codex => Provider::Claude,
    }
}

/// Whether `program` resolves on `PATH`.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
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
    let code = exit_code(launch(&account, &cwd, &args, None).await?);
    if code == 0 {
        eprintln!("quotatlas: {account} is ready: `quotatlas {} --account {}`", provider.program(), account.label);
    }
    Ok(code)
}

/// The account with the most room among `candidates`. Accounts whose room is
/// unknown come after the ones known to have room, in list order; accounts
/// known to be full are left out.
async fn pick(candidates: Vec<Account>, first_run: bool) -> Option<Account> {
    // A first run with a single account has nothing to choose between, and
    // reading Codex's room would only delay the start.
    if first_run && candidates.len() == 1 {
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

/// Run the CLI to its end. With `watch_since`, the session it writes is
/// checked while it runs, and the CLI is stopped once it shows the account's
/// limit: there is nothing more that account can do.
async fn launch(
    account: &Account,
    cwd: &Path,
    args: &[OsString],
    watch_since: Option<SystemTime>,
) -> anyhow::Result<ExitStatus> {
    let program = account.provider.program();
    let (before, after) = added_args(account, cwd, args);
    // What Quotatlas adds goes ahead of a `--`, after which the CLI would take
    // it for a prompt.
    let split = args.iter().position(|word| word == "--").unwrap_or(args.len());
    let saved = watch_since.and_then(|_| terminal::Saved::take());
    let mut child = tokio::process::Command::new(program)
        .args(before)
        .args(&args[..split])
        .args(after)
        .args(&args[split..])
        .envs(account.env())
        .spawn()
        .with_context(|| format!("starting `{program}`; is it installed and on PATH?"))?;
    let (Some(since), Some(saved)) = (watch_since, saved) else {
        return Ok(child.wait().await?);
    };
    let mut seen: Option<(PathBuf, SystemTime, u64)> = None;
    loop {
        tokio::select! {
            status = child.wait() => return Ok(status?),
            _ = tokio::time::sleep(WATCH_INTERVAL) => {}
        }
        let Some(session) = sessions::latest(account, cwd, since) else { continue };
        let Ok(meta) = std::fs::metadata(&session.path) else { continue };
        let stamp = (session.path.clone(), meta.modified().unwrap_or(since), meta.len());
        if seen.as_ref() == Some(&stamp) {
            continue;
        }
        seen = Some(stamp);
        let limited = sessions::limit_message(&session, since)
            .is_some_and(|message| sessions::is_account_limit(account.provider, &message));
        if limited {
            let status = terminal::stop(&mut child, STOP_GRACE).await?;
            saved.restore();
            return Ok(status);
        }
    }
}

/// What Quotatlas adds around the user's arguments, as (before, after): the
/// shared-memory server for this run, and for Claude the status line quota is
/// read through. Nothing is written to the CLI's own configuration.
///
/// Codex takes `-c` overrides ahead of any subcommand. Claude's
/// `--mcp-config` takes every value up to the next option, so it goes last,
/// where it cannot swallow a prompt.
fn added_args(account: &Account, cwd: &Path, args: &[OsString]) -> (Vec<OsString>, Vec<OsString>) {
    match account.provider {
        Provider::Codex => {
            let (server, server_args) = mcp::server_command("codex");
            let toml = |value: serde_json::Value| value.to_string();
            let key = format!("mcp_servers.{}", mcp::SERVER_NAME);
            // Memory tools run without a prompt, as the desktop app runs them:
            // they only read and write the project's own record.
            let before = [
                format!("{key}.command={}", toml(server.into())),
                format!("{key}.args={}", toml(server_args.into())),
                format!("{key}.default_tools_approval_mode=\"approve\""),
            ];
            (before.into_iter().flat_map(|value| ["-c".to_string(), value]).map(OsString::from).collect(), Vec::new())
        }
        Provider::Claude => {
            let (server, server_args) = mcp::server_command("claude-code");
            let config = serde_json::json!({
                "mcpServers": { mcp::SERVER_NAME: { "type": "stdio", "command": server, "args": server_args } }
            });
            let mut after: Vec<OsString> = status_line_args(account, cwd, args);
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

/// Claude reports quota through its status line. A profile has Quotatlas's
/// script in its own settings; otherwise the script is passed for this run,
/// running the account's own status line after it when there is one.
///
/// Nothing is passed when the user's arguments already decide the settings
/// (a second `--settings` would replace theirs; `--restricted` and
/// `--setting-sources` narrow what may run) or when the project sets a status
/// line, which Claude runs under its own trust rules.
fn status_line_args(account: &Account, cwd: &Path, args: &[OsString]) -> Vec<OsString> {
    if let Some(home) = &account.home {
        if let Err(e) = claude::install_statusline(home) {
            eprintln!("quotatlas: no quota for {account}: {e}");
        }
    }
    let theirs_decide = args.iter().take_while(|w| *w != "--").any(|word| {
        let word = word.to_string_lossy();
        ["--settings", "--setting-sources", "--restricted"]
            .iter()
            .any(|flag| word == *flag || word.starts_with(&format!("{flag}=")))
    });
    if theirs_decide || claude::project_sets_status_line(cwd) {
        return Vec::new();
    }
    let Some(user_settings) = account.cli_home().map(|home| home.join("settings.json")) else {
        return Vec::new();
    };
    let theirs = claude::user_status_line(&user_settings);
    if account.home.is_some() && theirs.is_none() {
        return Vec::new();
    }
    match claude::reading_dir(account).map(|dir| claude::write_script(&dir)) {
        Some(Ok(script)) => {
            vec!["--settings".into(), claude::status_line_settings(&script, theirs.as_ref()).to_string().into()]
        }
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

/// Stopping a CLI that is drawing in the terminal, and handing the terminal
/// back in the state it was in before.
#[cfg(unix)]
mod terminal {
    use std::process::{ExitStatus, Stdio};
    use std::time::Duration;

    /// The terminal's settings before the CLI started.
    pub struct Saved(String);

    impl Saved {
        /// `None` without a controlling terminal, which means nothing to restore.
        pub fn take() -> Option<Self> {
            let tty = std::fs::File::open("/dev/tty").ok()?;
            let out = std::process::Command::new("stty").arg("-g").stdin(tty).stderr(Stdio::null()).output().ok()?;
            out.status.success().then(|| Self(String::from_utf8_lossy(&out.stdout).trim().to_string()))
        }

        /// Put the settings back and switch off the modes a TUI turns on
        /// (alternate screen, hidden cursor, bracketed paste, mouse and focus
        /// reporting, keyboard enhancements), which a stopped one never did.
        pub fn restore(&self) {
            if let Ok(tty) = std::fs::File::open("/dev/tty") {
                let _ = std::process::Command::new("stty").arg(&self.0).stdin(tty).status();
            }
            eprint!("\x1b[?1049l\x1b[?25h\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1004l\x1b[<u\r\n");
        }
    }

    /// Ask the CLI to exit as Ctrl-C would, then insist: SIGINT, SIGTERM,
    /// SIGKILL, each after `grace`. Like Ctrl-C, each signal goes to the CLI
    /// and every descendant in its process group, so a `claude` that is a
    /// wrapper script does not leave the real CLI running. Quotatlas shares
    /// that group and is left out.
    pub async fn stop(child: &mut tokio::process::Child, grace: Duration) -> std::io::Result<ExitStatus> {
        let group = child.id().map(foreground_tree).unwrap_or_default();
        for signal in ["-INT", "-TERM", "-KILL"] {
            if !group.is_empty() {
                let _ = std::process::Command::new("kill")
                    .arg(signal)
                    .args(group.iter().map(u32::to_string))
                    .stderr(Stdio::null())
                    .status();
            }
            if let Ok(status) = tokio::time::timeout(grace, child.wait()).await {
                return status;
            }
        }
        child.kill().await?;
        child.wait().await
    }

    /// `pid` and its descendants that share its process group.
    fn foreground_tree(pid: u32) -> Vec<u32> {
        let rows: Vec<(u32, u32, u32)> = std::process::Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,pgid="])
            .output()
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter_map(|line| {
                        let mut fields = line.split_whitespace().map(|field| field.parse::<u32>().ok());
                        Some((fields.next()??, fields.next()??, fields.next()??))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let group_of = |p: u32| rows.iter().find(|row| row.0 == p).map(|row| row.2);
        let group = group_of(pid);
        descendants(pid, &rows).into_iter().filter(|p| group_of(*p) == group).collect()
    }

    /// `pid` and every process under it, parents first.
    pub(super) fn descendants(pid: u32, rows: &[(u32, u32, u32)]) -> Vec<u32> {
        let mut found = vec![pid];
        let mut index = 0;
        while index < found.len() {
            let parent = found[index];
            for row in rows {
                if row.1 == parent && !found.contains(&row.0) {
                    found.push(row.0);
                }
            }
            index += 1;
        }
        found
    }
}

/// Windows has no transcript watch: the handoff waits for the CLI to exit.
#[cfg(windows)]
mod terminal {
    use std::process::ExitStatus;
    use std::time::Duration;

    pub struct Saved;

    impl Saved {
        pub fn take() -> Option<Self> {
            None
        }

        pub fn restore(&self) {}
    }

    pub async fn stop(child: &mut tokio::process::Child, _grace: Duration) -> std::io::Result<ExitStatus> {
        child.kill().await?;
        child.wait().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn claudes_mode_follows_print() {
        assert_eq!(FirstRun::new(Provider::Claude, words(&["-p", "task"])).mode(), Mode::Batch);
        assert_eq!(FirstRun::new(Provider::Claude, words(&["task", "--", "-p"])).mode(), Mode::Interactive);
    }

    #[test]
    fn a_resume_keeps_the_mode_and_the_options() {
        let options = words(&["--model", "opus"]);
        assert_eq!(
            resume_args(Provider::Claude, Mode::Interactive, &options, "s1"),
            words(&["--model", "opus", "--resume", "s1", "--", CONTINUE_PROMPT])
        );
        assert_eq!(
            resume_args(Provider::Codex, Mode::Batch, &words(&["--skip-git-repo-check"]), "t1"),
            words(&["exec", "--skip-git-repo-check", "resume", "t1", CONTINUE_PROMPT])
        );
    }

    #[test]
    fn a_fresh_start_carries_the_brief_in_the_same_mode() {
        assert_eq!(fresh_args(Provider::Codex, Mode::Interactive, &[], "brief"), words(&["brief"]));
        assert_eq!(fresh_args(Provider::Codex, Mode::Batch, &[], "brief"), words(&["exec", "brief"]));
        assert_eq!(fresh_args(Provider::Claude, Mode::Batch, &[], "brief"), words(&["-p", "--", "brief"]));
        assert_eq!(
            fresh_args(Provider::Claude, Mode::Batch, &words(&["-p", "--add-dir", "x"]), "brief"),
            words(&["-p", "--add-dir", "x", "--", "brief"])
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_tree_under_a_process_includes_grandchildren_and_nothing_else() {
        // (pid, ppid, pgid): 10 is the CLI, 11 a wrapper's child, 12 its
        // child, 20 unrelated.
        let rows = [(10, 1, 10), (11, 10, 10), (12, 11, 10), (20, 1, 20)];
        assert_eq!(terminal::descendants(10, &rows), [10, 11, 12]);
    }

    #[test]
    fn another_cli_gets_none_of_the_first_runs_options() {
        let first = FirstRun::new(Provider::Claude, words(&["--model", "opus"]));
        assert!(first.options_for(Provider::Codex).is_empty());
    }
}
