//! The `quotatlas` command: run Claude Code or Codex with shared memory and
//! with a usage limit turned into a handoff to another account, serve that
//! memory to any MCP agent, show every account's quota, and open folders in
//! the desktop app.

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use quotatlas::accounts::{self, Account, Provider};
use quotatlas::quota::{self, claude, codex, QuotaWindow};
use quotatlas::run::{self, Run};

#[derive(Parser)]
#[command(name = "quotatlas", version, about = "Every coding agent. Every account. One memory.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run Claude Code. When a session stops on its usage limit, resume it on another account.
    Claude(AgentArgs),
    /// Run Codex. When a session stops on its usage limit, resume it on another account.
    Codex(AgentArgs),
    /// Show what every account has left.
    Quota,
    /// Add an account: another Claude Code or Codex login, and sign it in.
    Login {
        /// `claude` or `codex`.
        provider: Provider,
        /// A name for the account, e.g. `work`.
        name: String,
    },
    /// Serve the shared-memory tools over stdio, for an agent's MCP configuration.
    Mcp {
        /// Recorded as the writer of what this session remembers.
        #[arg(long, default_value = "mcp")]
        agent: String,
    },
    /// Open a folder in the Quotatlas desktop app.
    Open {
        /// The folder; the current one when left out.
        path: Option<PathBuf>,
    },
    /// `quotatlas <folder>`, the desktop app's shell helper usage, opens it too.
    #[command(external_subcommand)]
    Folder(Vec<OsString>),
}

#[derive(clap::Args)]
struct AgentArgs {
    /// The account to start on, by name; the one with the most room when left out.
    #[arg(long)]
    account: Option<String>,
    /// Passed to the agent CLI as given.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<OsString>,
}

#[tokio::main]
async fn main() {
    let code = match dispatch(Cli::parse().command).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("quotatlas: {error:#}");
            1
        }
    };
    std::process::exit(code);
}

async fn dispatch(command: Command) -> anyhow::Result<i32> {
    match command {
        Command::Claude(agent) => run::run(Run { provider: Provider::Claude, account: agent.account, args: agent.args }).await,
        Command::Codex(agent) => run::run(Run { provider: Provider::Codex, account: agent.account, args: agent.args }).await,
        Command::Quota => {
            print_quota().await;
            Ok(0)
        }
        Command::Login { provider, name } => run::login(provider, &name).await,
        Command::Mcp { agent } => {
            let cwd = std::env::current_dir().context("reading the current directory")?;
            quotatlas::mcp::serve(&cwd, &agent).await?;
            Ok(0)
        }
        Command::Open { path } => open(path),
        Command::Folder(words) => match words.as_slice() {
            [folder] if std::path::Path::new(folder).is_dir() => open(Some(PathBuf::from(folder))),
            _ => anyhow::bail!(
                "unknown command `{}`; `quotatlas --help` lists them",
                words.first().map(|word| word.to_string_lossy()).unwrap_or_default()
            ),
        },
    }
}

/// One line per account: the room left in its tightest window, then every window.
async fn print_quota() {
    let now = quota::now_secs();
    for provider in Provider::ALL {
        for account in accounts::list(provider) {
            let windows = match windows_of(&account).await {
                Ok(windows) if !windows.is_empty() => windows,
                Ok(_) => {
                    let hint = match provider {
                        Provider::Claude if account.home.is_none() => {
                            "no reading yet: use it once through `quotatlas claude`".to_string()
                        }
                        Provider::Claude => format!("no reading yet: use it once through `quotatlas claude --account {}`", account.label),
                        Provider::Codex => "no windows reported".to_string(),
                    };
                    println!("{:<7} {:<16} {hint}", provider.program(), account.label);
                    continue;
                }
                Err(reason) => {
                    println!("{:<7} {:<16} {reason}", provider.program(), account.label);
                    continue;
                }
            };
            let left = quota::headroom(&windows, now).map_or("?".to_string(), |left| format!("{left:.0}% left"));
            let detail: Vec<String> = windows.iter().map(|window| describe(window, now)).collect();
            println!("{:<7} {:<16} {left:<9} {}", provider.program(), account.label, detail.join(" · "));
        }
    }
}

async fn windows_of(account: &Account) -> Result<Vec<QuotaWindow>, String> {
    match account.provider {
        Provider::Claude => Ok(claude::reading_dir(account)
            .and_then(|dir| claude::read(&dir))
            .map(|reading| reading.windows)
            .unwrap_or_default()),
        Provider::Codex => match codex::read(account.home.as_deref()).await {
            Ok(reading) => Ok(reading.windows),
            Err(codex::CodexError::SignedOut) => Err("not signed in".into()),
            Err(codex::CodexError::NotInstalled) => Err("`codex` is not on PATH".into()),
            Err(codex::CodexError::Failed(reason)) => Err(reason),
        },
    }
}

fn describe(window: &QuotaWindow, now: i64) -> String {
    let used = window.used_at(now);
    match window.resets_at.filter(|reset| *reset > now) {
        Some(reset) => format!("{} {used:.0}% (resets in {})", window.label, span(reset - now)),
        None => format!("{} {used:.0}%", window.label),
    }
}

/// `2d 3h`, `4h 10m` or `12m`.
fn span(seconds: i64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds % 86_400 / 3_600, seconds % 3_600 / 60);
    match (days, hours) {
        (0, 0) => format!("{minutes}m"),
        (0, _) => format!("{hours}h {minutes}m"),
        _ => format!("{days}d {hours}h"),
    }
}

/// Hand the folder to the desktop app, as a new window.
fn open(path: Option<PathBuf>) -> anyhow::Result<i32> {
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let folder = std::fs::canonicalize(&path).with_context(|| format!("no folder at {}", path.display()))?;
    anyhow::ensure!(folder.is_dir(), "{} is not a folder", folder.display());
    open_in_app(&folder)
}

/// `-n` starts a fresh instance so the folder arrives as an argument; the
/// app's single-instance handler forwards it to the running one.
#[cfg(target_os = "macos")]
fn open_in_app(folder: &std::path::Path) -> anyhow::Result<i32> {
    let status = std::process::Command::new("open")
        .args(["-na", "Quotatlas", "--args"])
        .arg(folder)
        .status()
        .context("running `open`")?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(not(target_os = "macos"))]
fn open_in_app(_folder: &std::path::Path) -> anyhow::Result<i32> {
    anyhow::bail!("opening folders in the desktop app is macOS-only for now")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_read_at_a_glance() {
        assert_eq!(span(12 * 60), "12m");
        assert_eq!(span(4 * 3_600 + 10 * 60), "4h 10m");
        assert_eq!(span(2 * 86_400 + 3 * 3_600), "2d 3h");
    }

    #[test]
    fn a_folder_in_place_of_a_command_is_opened() {
        let cli = Cli::try_parse_from(["quotatlas", "."]).unwrap();
        assert!(matches!(cli.command, Command::Folder(words) if words == ["."]));
    }

    #[test]
    fn agent_arguments_pass_through_untouched() {
        let cli = Cli::try_parse_from(["quotatlas", "claude", "--account", "work", "--model", "opus", "-c"]).unwrap();
        let Command::Claude(agent) = cli.command else { panic!("not claude") };
        assert_eq!(agent.account.as_deref(), Some("work"));
        assert_eq!(agent.args, ["--model", "opus", "-c"]);
    }
}
