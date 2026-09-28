//! Claude quota, read through Claude Code's own status line (ADR Q-0002).
//!
//! Claude Code documents one machine-readable view of subscription limits:
//! the JSON it pipes to a statusLine command, whose `rate_limits.five_hour`
//! and `rate_limits.seven_day` carry `used_percentage` and `resets_at`
//! (code.claude.com/docs/en/statusline.md). A Quotatlas account's profile gets
//! a statusLine script that saves that JSON next to it, and Quotatlas reads the
//! saved copy. The default login's settings are not edited; `quotatlas claude`
//! passes the script per run instead, when the user has no status line of
//! their own. The account's login is never read. The figures are as fresh as
//! the account's last turn; the saved file's modification time says how fresh.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::QuotaWindow;
use crate::accounts::{self, Account};

/// Where the status line script saves Claude Code's status JSON.
pub const RATE_LIMITS_FILE: &str = "quotatlas-rate-limits.json";
/// The script itself, inside the profile home it serves.
const SCRIPT_FILE: &str = "quotatlas-statusline.sh";
/// Claude Code's user settings file inside a `CLAUDE_CONFIG_DIR`.
const SETTINGS_FILE: &str = "settings.json";

/// Writes stdin to a temporary file and renames it over the saved copy, so
/// Quotatlas never reads a half-written file. Given an argument, it runs that
/// as the user's own status line on the same JSON and prints what it prints;
/// otherwise a fixed status text. Nothing user-controlled is interpolated into
/// the script: the user's command arrives as an argument.
const SCRIPT: &str = r#"#!/bin/sh
# Written by Quotatlas for this Claude Code account. Claude Code pipes its
# status JSON to this script; Quotatlas reads the rate_limits in the saved
# copy to show the account's quota. The account's login is never read.
dir=$(dirname "$0")
tmp="$dir/.quotatlas-rate-limits.$$"
umask 077
if cat > "$tmp"; then
  if [ -n "$1" ]; then
    sh -c "$1" < "$tmp"
  else
    printf 'Quotatlas\n'
  fi
  mv -f "$tmp" "$dir/quotatlas-rate-limits.json"
else
  rm -f "$tmp"
fi
"#;

/// A saved reading: the windows and when Claude Code produced them.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeReading {
    pub windows: Vec<QuotaWindow>,
    /// Unix seconds, from the saved file's modification time.
    pub updated_at: Option<i64>,
}

fn script_path(profile_home: &Path) -> PathBuf {
    profile_home.join(SCRIPT_FILE)
}

/// Where the default login's readings are saved. Not in `~/.claude`: the
/// default login's files are the user's, so the script lives in Quotatlas's
/// own data dir and is handed to Claude Code per run (see `status_line_settings`).
pub fn default_reading_dir() -> Option<PathBuf> {
    accounts::app_data_dir().map(|dir| dir.join("claude-default"))
}

/// Where `account`'s readings are saved.
pub fn reading_dir(account: &Account) -> Option<PathBuf> {
    account.home.clone().or_else(default_reading_dir)
}

/// Write the status line script into `dir` and return its path.
pub fn write_script(dir: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let script = script_path(dir);
    write_atomic(&script, SCRIPT.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(script)
}

/// A settings object for Claude Code's `--settings` flag that points the
/// status line at `script`. With `theirs`, the user's own status line, the
/// script runs their command after saving the JSON, and their other fields
/// (such as `padding`) are kept.
pub fn status_line_settings(script: &Path, theirs: Option<&Value>) -> Value {
    let mut command = shell_quote(script);
    let mut line = json!({ "type": "command" });
    if let Some(theirs) = theirs {
        if let Some(own) = theirs.get("command").and_then(Value::as_str) {
            command = format!("{command} {}", shell_quote(Path::new(own)));
        }
        if let (Some(line), Some(fields)) = (line.as_object_mut(), theirs.as_object()) {
            for (key, value) in fields {
                line.insert(key.clone(), value.clone());
            }
        }
    }
    line["command"] = Value::String(command);
    json!({ "statusLine": line })
}

/// The status line the account's own settings set, unless it is the script
/// Quotatlas installed there.
pub fn user_status_line(user_settings: &Path) -> Option<Value> {
    let line = settings_status_line(user_settings)?;
    let ours = user_settings.parent().map(|home| shell_quote(&script_path(home)));
    (line.get("command").and_then(Value::as_str) != ours.as_deref()).then_some(line)
}

/// Whether the project in `cwd` sets a status line of its own. Claude Code
/// runs that one under its own trust rules; Quotatlas does not lift it into a
/// flag, where those rules (and `--restricted`) would no longer apply.
pub fn project_sets_status_line(cwd: &Path) -> bool {
    [".claude/settings.local.json", ".claude/settings.json"]
        .iter()
        .any(|file| settings_status_line(&cwd.join(file)).is_some())
}

fn settings_status_line(file: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(file).ok()?;
    serde_json::from_str::<Value>(&raw).ok()?.get("statusLine").cloned()
}

/// Quote a path for the shell that runs the statusLine command. The app
/// config dir is under "Application Support", so the space is the common case.
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// Install the status line script into a Quotatlas-managed profile and point
/// the profile's `settings.json` at it. Idempotent. A statusLine the user
/// configured themselves in this profile is left alone, and quota for the
/// account then stays unknown.
pub fn install_statusline(profile_home: &Path) -> io::Result<()> {
    let script = write_script(profile_home)?;

    let settings_path = profile_home.join(SETTINGS_FILE);
    let mut settings = match std::fs::read_to_string(&settings_path) {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(Value::Object(map)) => map,
            // Not ours to repair: a settings file the CLI cannot parse either
            // is the user's to fix, and overwriting it would lose their edits.
            _ => return Ok(()),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Map::new(),
        Err(e) => return Err(e),
    };
    let command = shell_quote(&script);
    let ours = settings
        .get("statusLine")
        .and_then(|line| line.get("command"))
        .and_then(Value::as_str)
        .is_some_and(|existing| existing == command);
    if settings.contains_key("statusLine") && !ours {
        return Ok(());
    }
    settings.insert("statusLine".into(), json!({ "type": "command", "command": command }));
    let body = serde_json::to_string_pretty(&Value::Object(settings)).map_err(io::Error::other)?;
    write_atomic(&settings_path, body.as_bytes())
}

/// Whether Claude Code has been told to trust `cwd` under `account`: each
/// login keeps its own answers, in `.claude.json` beside its settings (in the
/// home directory for the default login). `None` when that file cannot be read.
pub fn trusts_folder(account: &Account, cwd: &Path) -> Option<bool> {
    let config = match (&account.home, std::env::var_os("CLAUDE_CONFIG_DIR")) {
        (Some(home), _) => home.join(".claude.json"),
        (None, Some(dir)) if !dir.is_empty() => PathBuf::from(dir).join(".claude.json"),
        (None, _) => dirs::home_dir()?.join(".claude.json"),
    };
    let settings: Value = serde_json::from_str(&std::fs::read_to_string(config).ok()?).ok()?;
    let key = cwd.to_string_lossy();
    Some(settings.pointer("/projects").and_then(|projects| projects.get(key.as_ref())).and_then(|p| p.get("hasTrustDialogAccepted")) == Some(&Value::Bool(true)))
}

/// Write via a sibling temp file and a rename.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("quotatlas-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// The saved reading, or `None` before the account's first turn.
pub fn read(profile_home: &Path) -> Option<ClaudeReading> {
    let path = profile_home.join(RATE_LIMITS_FILE);
    let raw = std::fs::read_to_string(&path).ok()?;
    let status: Value = serde_json::from_str(&raw).ok()?;
    let updated_at = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_secs() as i64);
    Some(ClaudeReading { windows: windows_from_status(&status), updated_at })
}

/// The windows in a status JSON's `rate_limits`. Absent before the session's
/// first reply and for accounts without a subscription.
fn windows_from_status(status: &Value) -> Vec<QuotaWindow> {
    let Some(limits) = status.get("rate_limits") else {
        return Vec::new();
    };
    [
        ("five_hour", "session", "Session", Some(300)),
        ("seven_day", "weekly", "Weekly", Some(10_080)),
        ("spend_limit", "spend", "Spend limit", None),
    ]
    .into_iter()
    .filter_map(|(key, id, label, minutes)| {
        let window = limits.get(key)?;
        let used = window.get("used_percentage")?.as_f64()?;
        Some(QuotaWindow {
            id: id.to_string(),
            label: label.to_string(),
            used_percent: used.clamp(0.0, 100.0),
            window_minutes: minutes,
            resets_at: window.get("resets_at").and_then(Value::as_i64),
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(limits: Value) -> Value {
        json!({ "session_id": "s", "model": { "id": "m" }, "rate_limits": limits })
    }

    #[test]
    fn five_hour_and_seven_day_become_session_and_weekly() {
        let windows = windows_from_status(&status(json!({
            "five_hour": { "used_percentage": 9, "resets_at": 1790000000 },
            "seven_day": { "used_percentage": 71.5, "resets_at": 1790500000 }
        })));
        assert_eq!(windows.len(), 2);
        assert_eq!((windows[0].label.as_str(), windows[0].used_percent), ("Session", 9.0));
        assert_eq!(windows[0].window_minutes, Some(300));
        assert_eq!((windows[1].label.as_str(), windows[1].used_percent), ("Weekly", 71.5));
        assert_eq!(windows[1].resets_at, Some(1790500000));
    }

    #[test]
    fn a_status_without_rate_limits_has_no_windows() {
        assert!(windows_from_status(&json!({ "session_id": "s" })).is_empty());
    }

    #[test]
    fn paths_are_single_quoted_for_the_shell() {
        let quoted = shell_quote(Path::new("/Users/a/Library/Application Support/x/it's"));
        assert_eq!(quoted, r"'/Users/a/Library/Application Support/x/it'\''s'");
    }

    #[test]
    fn installing_points_a_fresh_profile_at_the_script_and_is_idempotent() {
        let home = tempfile::tempdir().unwrap();
        install_statusline(home.path()).unwrap();
        install_statusline(home.path()).unwrap();

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(home.path().join(SETTINGS_FILE)).unwrap())
                .unwrap();
        assert_eq!(settings["statusLine"]["type"], "command");
        assert_eq!(settings["statusLine"]["command"], shell_quote(&script_path(home.path())));
        assert!(std::fs::read_to_string(script_path(home.path())).unwrap().starts_with("#!/bin/sh"));
    }

    #[test]
    fn a_status_line_the_user_set_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let mine = json!({ "statusLine": { "type": "command", "command": "~/bin/mine" }, "theme": "dark" });
        std::fs::write(home.path().join(SETTINGS_FILE), mine.to_string()).unwrap();
        install_statusline(home.path()).unwrap();
        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(home.path().join(SETTINGS_FILE)).unwrap())
                .unwrap();
        assert_eq!(settings, mine);
    }

    #[test]
    fn other_settings_survive_the_install() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(SETTINGS_FILE), r#"{ "theme": "dark" }"#).unwrap();
        install_statusline(home.path()).unwrap();
        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(home.path().join(SETTINGS_FILE)).unwrap())
                .unwrap();
        assert_eq!(settings["theme"], "dark");
        assert!(settings.get("statusLine").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn the_script_runs_the_users_own_status_line_on_the_same_json() {
        let home = tempfile::tempdir().unwrap();
        let script = write_script(home.path()).unwrap();
        let output = std::process::Command::new(&script)
            .arg("cat; printf ' | mine'")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.take().unwrap().write_all(br#"{"rate_limits":{}}"#)?;
                child.wait_with_output()
            })
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), r#"{"rate_limits":{}} | mine"#);
        assert!(home.path().join(RATE_LIMITS_FILE).exists());
    }

    #[test]
    fn the_users_status_line_is_chained_ours_is_not_theirs_and_a_projects_is_left_to_claude() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let user = home.path().join(SETTINGS_FILE);
        assert_eq!(user_status_line(&user), None);

        install_statusline(home.path()).unwrap();
        assert_eq!(user_status_line(&user), None);

        let mine = json!({ "type": "command", "command": "~/bin/line", "padding": 0 });
        std::fs::write(&user, json!({ "statusLine": mine }).to_string()).unwrap();
        let theirs = user_status_line(&user).unwrap();
        let settings = status_line_settings(Path::new("/q/s.sh"), Some(&theirs));
        assert_eq!(settings["statusLine"]["command"], "'/q/s.sh' '~/bin/line'");
        assert_eq!(settings["statusLine"]["padding"], 0);
        assert_eq!(status_line_settings(Path::new("/q/s.sh"), None)["statusLine"]["command"], "'/q/s.sh'");

        assert!(!project_sets_status_line(project.path()));
        std::fs::create_dir_all(project.path().join(".claude")).unwrap();
        std::fs::write(project.path().join(".claude/settings.json"), json!({ "statusLine": mine }).to_string()).unwrap();
        assert!(project_sets_status_line(project.path()));
    }

    #[test]
    fn trust_is_read_per_login_from_its_own_config() {
        let home = tempfile::tempdir().unwrap();
        let account = Account { id: "claude-acp-p".into(), provider: crate::accounts::Provider::Claude, label: "p".into(), home: Some(home.path().to_path_buf()) };
        assert_eq!(trusts_folder(&account, Path::new("/w")), None);
        std::fs::write(home.path().join(".claude.json"), json!({ "projects": { "/w": { "hasTrustDialogAccepted": true }, "/x": {} } }).to_string()).unwrap();
        assert_eq!(trusts_folder(&account, Path::new("/w")), Some(true));
        assert_eq!(trusts_folder(&account, Path::new("/x")), Some(false));
        assert_eq!(trusts_folder(&account, Path::new("/y")), Some(false));
    }

    #[cfg(unix)]
    #[test]
    fn the_script_saves_stdin_and_prints_a_fixed_status() {
        let home = tempfile::tempdir().unwrap();
        install_statusline(home.path()).unwrap();
        let payload = status(json!({ "five_hour": { "used_percentage": 42, "resets_at": 1 } }));
        let output = std::process::Command::new(script_path(home.path()))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.take().unwrap().write_all(payload.to_string().as_bytes())?;
                child.wait_with_output()
            })
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), "Quotatlas\n");
        let reading = read(home.path()).unwrap();
        assert_eq!(reading.windows[0].used_percent, 42.0);
        assert!(reading.updated_at.is_some());
    }
}
