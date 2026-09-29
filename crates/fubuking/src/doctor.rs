//! What FubuKing can tell about its own footing: each agent CLI's version
//! against the versions its handoff is tested with, and, for
//! `fubuking doctor`, what it reads in this folder.
//!
//! The session formats a handoff reads are no documented interface of either
//! CLI (see `sessions`), so a CLI outside the tested versions may have
//! changed them. That is said once per version, not on every run.

use std::fmt::Write as _;
use std::io::IsTerminal;
use std::path::Path;
use std::time::SystemTime;

use crate::accounts::{self, Provider};
use crate::sessions::{self, Ending};

/// Where the versions already announced as untested are kept.
const ANNOUNCED_FILE: &str = "untested-cli-versions";

/// A CLI version, `major.minor.patch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// The first `x.y.z` in a CLI's `--version` output, a pre-release suffix
    /// dropped: `2.1.283 (Claude Code)`, `codex-cli 0.157.1`.
    pub fn find(text: &str) -> Option<Self> {
        text.split_whitespace().find_map(|word| {
            let mut parts = word.split(['-', '+']).next()?.split('.').map(|part| part.parse::<u32>().ok());
            let version = Self(parts.next()??, parts.next()??, parts.next()??);
            parts.next().is_none().then_some(version)
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// The oldest and newest versions of `provider`'s CLI that FubuKing's handoff
/// is tested with: real runs, and the records `tests/handoff` replays. Move
/// the newest when those records are refreshed for a new release.
pub fn tested(provider: Provider) -> (Version, Version) {
    match provider {
        Provider::Claude => (Version(2, 1, 273), Version(2, 1, 283)),
        Provider::Codex => (Version(0, 147, 0), Version(0, 157, 1)),
    }
}

/// The installed CLI's version, when it says.
pub fn installed(provider: Provider) -> Option<Version> {
    let out = atlas_process::command(crate::executable(provider.program())).arg("--version").output().ok()?;
    Version::find(&String::from_utf8_lossy(&out.stdout))
}

/// Say once per version when the installed CLI is not one FubuKing's handoff
/// is tested with.
pub fn warn_if_untested(provider: Provider) {
    let Some(note) = installed(provider).and_then(|version| untested(provider, version)) else { return };
    let announced = accounts::app_data_dir().map(|dir| dir.join(ANNOUNCED_FILE));
    if first_time(announced.as_deref(), &note.0) {
        eprintln!("fubuking: {}", note.1);
    }
}

/// `(key, note)` when `version` is outside the tested range: the key names
/// the version, the note says what that means.
fn untested(provider: Provider, version: Version) -> Option<(String, String)> {
    let (oldest, newest) = tested(provider);
    let name = provider.name();
    let note = if version > newest {
        format!(
            "{name} {version} is newer than {newest}, the last version FubuKing's handoff is tested with. \
             If a usage limit goes unnoticed, `fubuking doctor` shows what FubuKing reads."
        )
    } else if version < oldest {
        format!(
            "{name} {version} is older than {oldest}, the first version FubuKing's handoff is tested with. \
             Update it if a usage limit goes unnoticed."
        )
    } else {
        return None;
    };
    Some((format!("{} {version}", provider.program()), note))
}

/// Whether `key` is not in the list at `file` yet; it is from now on. Without
/// a place to keep the list, every time is the first.
fn first_time(file: Option<&Path>, key: &str) -> bool {
    let Some(file) = file else { return true };
    if std::fs::read_to_string(file).is_ok_and(|seen| seen.lines().any(|line| line == key)) {
        return false;
    }
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut list| std::io::Write::write_all(&mut list, format!("{key}\n").as_bytes()));
    true
}

/// `fubuking doctor`: the agent CLIs and their versions, each account's
/// latest session in `cwd` and how it ended, and whether a handoff can ask
/// here. Reads only; writes nothing.
pub fn report(cwd: &Path) -> String {
    let mut out = String::new();
    for provider in Provider::ALL {
        let (oldest, newest) = tested(provider);
        let found = match which::which(provider.program()) {
            Err(_) => "not found on PATH".to_string(),
            Ok(path) => {
                let version = installed(provider).map_or("version unknown".to_string(), |version| version.to_string());
                format!("{version} at {} (handoff tested with {oldest} to {newest})", path.display())
            }
        };
        let _ = writeln!(out, "{:<7} {found}", provider.program());
    }
    out.push('\n');
    let now = SystemTime::now();
    for provider in Provider::ALL {
        for account in accounts::list(provider) {
            let state = match sessions::latest(&account, cwd, SystemTime::UNIX_EPOCH) {
                Some(session) => {
                    let age = std::fs::metadata(&session.path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|at| now.duration_since(at).ok())
                        .map_or("?".to_string(), |elapsed| ago(elapsed.as_secs()));
                    format!("last session here {age} ago: {}", describe(sessions::ending(&session, SystemTime::UNIX_EPOCH)))
                }
                None => match sessions::stray(&account, cwd, SystemTime::UNIX_EPOCH) {
                    Some(path) if provider == Provider::Claude => {
                        format!("no session where FubuKing looks; one for this folder is at {}", path.display())
                    }
                    Some(path) => format!("no rollout for this folder FubuKing can read; the newest is {}", path.display()),
                    None => "no session here yet".to_string(),
                },
            };
            let _ = writeln!(out, "{:<7} {:<16} {state}", provider.program(), account.label);
        }
    }
    out.push('\n');
    out.push_str(if std::io::stdin().is_terminal() {
        "On a terminal: a usage limit is handed off here after asking.\n"
    } else {
        "No terminal: FubuKing hands off only when it can ask.\n"
    });
    out
}

fn describe(ending: Ending) -> String {
    match ending {
        Ending::Limit(message) => format!("ended on a usage limit ({message})"),
        Ending::Unread(said) => format!("ended on a limit in a form FubuKing does not read ({said})"),
        Ending::Other => "not on a usage limit".to_string(),
    }
}

/// `12m`, `5h` or `3d`.
fn ago(seconds: u64) -> String {
    match seconds {
        0..3_600 => format!("{}m", seconds / 60),
        3_600..172_800 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_read_from_what_each_cli_prints() {
        assert_eq!(Version::find("2.1.283 (Claude Code)"), Some(Version(2, 1, 283)));
        assert_eq!(Version::find("codex-cli 0.157.1\n"), Some(Version(0, 157, 1)));
        assert_eq!(Version::find("codex-cli 0.158.0-alpha.2.1"), Some(Version(0, 158, 0)));
        assert_eq!(Version::find("error: unknown option"), None);
        assert!(Version(2, 1, 290) > Version(2, 1, 283) && Version(0, 99, 0) < Version(0, 147, 0));
    }

    #[test]
    fn only_versions_outside_the_tested_range_are_untested() {
        let (oldest, newest) = tested(Provider::Claude);
        assert_eq!(untested(Provider::Claude, oldest), None);
        assert_eq!(untested(Provider::Claude, newest), None);
        let (key, note) = untested(Provider::Claude, Version(2, 1, 290)).unwrap();
        assert_eq!(key, "claude 2.1.290");
        assert!(note.starts_with("Claude Code 2.1.290 is newer than 2.1.283"), "{note}");
        assert!(untested(Provider::Codex, Version(0, 120, 0)).unwrap().1.contains("is older than 0.147.0"));
    }

    #[test]
    fn a_version_is_announced_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data").join(ANNOUNCED_FILE);
        assert!(first_time(Some(&file), "claude 2.1.290"));
        assert!(!first_time(Some(&file), "claude 2.1.290"));
        assert!(first_time(Some(&file), "codex 0.160.0"));
        assert!(first_time(None, "claude 2.1.290"));
    }

    #[test]
    fn ages_read_at_a_glance() {
        assert_eq!(ago(12 * 60), "12m");
        assert_eq!(ago(5 * 3_600), "5h");
        assert_eq!(ago(3 * 86_400), "3d");
    }
}
