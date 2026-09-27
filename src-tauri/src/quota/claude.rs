//! Claude quota, read through Claude Code's own status line (ADR Q-0002).
//!
//! Claude Code documents one machine-readable view of subscription limits:
//! the JSON it pipes to a statusLine command, whose `rate_limits.five_hour`
//! and `rate_limits.seven_day` carry `used_percentage` and `resets_at`
//! (code.claude.com/docs/en/statusline.md). A Quotatlas account's profile gets
//! a statusLine script that saves that JSON next to it, and Quotatlas reads the
//! saved copy. The account's login is never read. The figures are as fresh as
//! the account's last turn; the saved file's modification time says how fresh.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::QuotaWindow;

/// Where the status line script saves Claude Code's status JSON.
pub const RATE_LIMITS_FILE: &str = "quotatlas-rate-limits.json";
/// The script itself, inside the profile home it serves.
const SCRIPT_FILE: &str = "quotatlas-statusline.sh";
/// Claude Code's user settings file inside a `CLAUDE_CONFIG_DIR`.
const SETTINGS_FILE: &str = "settings.json";

/// Writes stdin to a temporary file and renames it over the saved copy, so
/// Quotatlas never reads a half-written file. It prints a fixed status text:
/// nothing user-controlled is ever interpolated into the script.
const SCRIPT: &str = r#"#!/bin/sh
# Written by Quotatlas for this Claude Code account. Claude Code pipes its
# status JSON to this script; Quotatlas reads the rate_limits in the saved
# copy to show the account's quota. The account's login is never read.
dir=$(dirname "$0")
tmp="$dir/.quotatlas-rate-limits.$$"
umask 077
if cat > "$tmp"; then
  mv -f "$tmp" "$dir/quotatlas-rate-limits.json"
else
  rm -f "$tmp"
fi
printf 'Quotatlas\n'
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
    let script = script_path(profile_home);
    write_atomic(&script, SCRIPT.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
    }

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
