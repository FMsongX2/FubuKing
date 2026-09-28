//! The `fubuking` command: run Claude Code or Codex with shared memory and
//! with a usage limit turned into a handoff to another account, serve that
//! memory to any MCP agent, and show every account's quota.

use std::ffi::OsString;

use anyhow::Context;
use clap::{Parser, Subcommand};
use fubuking::accounts::{self, Account, Provider};
use fubuking::quota::{self, claude, codex, QuotaWindow};
use fubuking::run::{self, Run};

#[derive(Parser)]
#[command(name = "fubuking", version, about = "Every coding agent. Every account. One memory.")]
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
    // `claude` and `codex` hand their arguments to the CLI word for word,
    // `--` included, which clap would consume; only a leading `--account`
    // is FubuKing's.
    let argv: Vec<OsString> = std::env::args_os().collect();
    let command = match agent_command(&argv) {
        Some(command) => command,
        None => Cli::parse().command,
    };
    let code = match dispatch(command).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("fubuking: {error:#}");
            1
        }
    };
    std::process::exit(code);
}

/// `fubuking claude|codex [--account <name>] <args...>`, read without clap.
/// `None` for anything else, and for `--help`, which clap answers.
fn agent_command(argv: &[OsString]) -> Option<Command> {
    let (which, rest) = (argv.get(1)?.to_str()?, argv.get(2..).unwrap_or_default());
    let make: fn(AgentArgs) -> Command = match which {
        "claude" => Command::Claude,
        "codex" => Command::Codex,
        _ => return None,
    };
    let first = rest.first().and_then(|word| word.to_str());
    let (account, args) = match first {
        Some("--help" | "-h") => return None,
        Some("--account") => (Some(rest.get(1)?.to_string_lossy().into_owned()), &rest[2..]),
        Some(word) if word.starts_with("--account=") => (Some(word["--account=".len()..].to_string()), &rest[1..]),
        _ => (None, rest),
    };
    Some(make(AgentArgs { account, args: args.to_vec() }))
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
            fubuking::mcp::serve(&cwd, &agent).await?;
            Ok(0)
        }
    }
}

/// One line per account: the room left in its tightest window, then every
/// window. Stops quietly when the reader goes away (`fubuking quota | head`).
async fn print_quota() {
    use std::io::Write;
    let now = quota::now_secs();
    let mut out = std::io::stdout().lock();
    for provider in Provider::ALL {
        for account in accounts::list(provider) {
            let windows = match windows_of(&account).await {
                Ok(windows) if !windows.is_empty() => windows,
                Ok(_) => {
                    let hint = match provider {
                        Provider::Claude if account.home.is_none() => {
                            "no reading yet: use it once through `fubuking claude`".to_string()
                        }
                        Provider::Claude => format!("no reading yet: use it once through `fubuking claude --account {}`", account.label),
                        Provider::Codex => "no windows reported".to_string(),
                    };
                    if writeln!(out, "{:<7} {:<16} {hint}", provider.program(), account.label).is_err() {
                        return;
                    }
                    continue;
                }
                Err(reason) => {
                    if writeln!(out, "{:<7} {:<16} {reason}", provider.program(), account.label).is_err() {
                        return;
                    }
                    continue;
                }
            };
            let left = quota::headroom(&windows, now).map_or("?".to_string(), |left| format!("{left:.0}% left"));
            let detail: Vec<String> = windows.iter().map(|window| describe(window, now)).collect();
            if writeln!(out, "{:<7} {:<16} {left:<9} {}", provider.program(), account.label, detail.join(" · ")).is_err() {
                return;
            }
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
    fn agent_arguments_keep_their_double_dash_and_only_a_leading_account_is_ours() {
        let argv = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        let Some(Command::Claude(agent)) = agent_command(&argv(&["fubuking", "claude", "--", "-x fix"])) else {
            panic!("not claude")
        };
        assert_eq!((agent.account, agent.args), (None, argv(&["--", "-x fix"])));
        let Some(Command::Codex(agent)) = agent_command(&argv(&["fubuking", "codex", "--account=side", "exec", "--account"])) else {
            panic!("not codex")
        };
        assert_eq!((agent.account.as_deref(), agent.args), (Some("side"), argv(&["exec", "--account"])));
        assert!(agent_command(&argv(&["fubuking", "claude", "--help"])).is_none());
        assert!(agent_command(&argv(&["fubuking", "quota"])).is_none());
    }

    #[test]
    fn agent_arguments_pass_through_untouched() {
        let cli = Cli::try_parse_from(["fubuking", "claude", "--account", "work", "--model", "opus", "-c"]).unwrap();
        let Command::Claude(agent) = cli.command else { panic!("not claude") };
        assert_eq!(agent.account.as_deref(), Some("work"));
        assert_eq!(agent.args, ["--model", "opus", "-c"]);
    }
}
