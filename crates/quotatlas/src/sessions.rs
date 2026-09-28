//! Agent session transcripts: where each CLI writes them, whether one stopped
//! on a usage limit, and copying one to another account so it resumes there.
//!
//! Both CLIs record a limit hit in the transcript itself. Claude Code writes
//! an assistant entry with `isApiErrorMessage: true` and `error: "rate_limit"`;
//! Codex ends the turn with a `task_complete` event whose
//! `error.codex_error_info` is `usage_limit_exceeded`. Neither format is a
//! documented interface, so when one changes a limit goes unnoticed: the run
//! simply ends, it never hands off by mistake.
//!
//! Resuming a copy works because each CLI looks a session up by id under its
//! own home: `<CLAUDE_CONFIG_DIR>/projects/<encoded cwd>/<id>.jsonl` and
//! `<CODEX_HOME>/sessions/<yyyy>/<mm>/<dd>/rollout-<time>-<id>.jsonl`. Checked
//! against Claude Code 2.1.273 and Codex 0.147.0; neither documents it.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::accounts::{Account, Provider};

/// How much of a transcript's end is read to find how it stopped. The last
/// assistant entry or turn event sits well inside it.
const TAIL_BYTES: u64 = 512 * 1024;

/// One CLI session on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub provider: Provider,
    pub id: String,
    pub path: PathBuf,
    /// `path` relative to the CLI home it lives in, which is where a copy goes
    /// under another account's home.
    pub relative: PathBuf,
}

/// The session `account` wrote to most recently for `cwd`, if it wrote one
/// after `since`.
pub fn latest(account: &Account, cwd: &Path, since: SystemTime) -> Option<Session> {
    let home = account.cli_home()?;
    match account.provider {
        Provider::Claude => latest_claude(&home, cwd, since),
        Provider::Codex => latest_codex(&home, cwd, since),
    }
}

fn latest_claude(home: &Path, cwd: &Path, since: SystemTime) -> Option<Session> {
    let dir = home
        .join("projects")
        .join(atlas_agent_transcript::encode_cwd(&cwd.to_string_lossy()));
    // ponytail: newest file wins, so two sessions of one account in one folder
    // at once can be mistaken for each other.
    let path = jsonl_files(&dir)
        .filter(|(_, modified)| *modified >= since)
        .max_by_key(|(_, modified)| *modified)?
        .0;
    session(Provider::Claude, home, path, |path| {
        path.file_stem().map(|stem| stem.to_string_lossy().into_owned())
    })
}

fn latest_codex(home: &Path, cwd: &Path, since: SystemTime) -> Option<Session> {
    let mut candidates: Vec<(PathBuf, SystemTime)> = Vec::new();
    let mut dirs = vec![home.join("sessions")];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if let Some(modified) = modified_since(&path, since) {
                candidates.push((path, modified));
            }
        }
    }
    candidates.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    let path = candidates
        .into_iter()
        .map(|(path, _)| path)
        .find(|path| codex_cwd(path).is_some_and(|recorded| same_dir(&recorded, cwd)))?;
    session(Provider::Codex, home, path, |path| {
        // `rollout-2026-09-27T22-01-12-<uuid>`: the id is the last five groups.
        let stem = path.file_stem()?.to_string_lossy().into_owned();
        let parts: Vec<&str> = stem.rsplitn(6, '-').collect();
        (parts.len() == 6).then(|| parts[..5].iter().rev().copied().collect::<Vec<_>>().join("-"))
    })
}

fn session(
    provider: Provider,
    home: &Path,
    path: PathBuf,
    id_of: impl Fn(&Path) -> Option<String>,
) -> Option<Session> {
    let id = id_of(&path)?;
    let relative = path.strip_prefix(home).ok()?.to_path_buf();
    Some(Session { provider, id, path, relative })
}

fn jsonl_files(dir: &Path) -> impl Iterator<Item = (PathBuf, SystemTime)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|path| {
            let modified = modified_since(&path, SystemTime::UNIX_EPOCH)?;
            Some((path, modified))
        })
}

/// The file's modification time, for a `.jsonl` file modified at or after `since`.
fn modified_since(path: &Path, since: SystemTime) -> Option<SystemTime> {
    if path.extension().is_none_or(|ext| ext != "jsonl") {
        return None;
    }
    let modified = std::fs::metadata(path).and_then(|meta| meta.modified()).ok()?;
    (modified >= since).then_some(modified)
}

/// The working directory a Codex rollout's first line records.
fn codex_cwd(path: &Path) -> Option<PathBuf> {
    let mut first = String::new();
    BufReader::new(File::open(path).ok()?).read_line(&mut first).ok()?;
    let meta: Value = serde_json::from_str(&first).ok()?;
    meta.pointer("/payload/cwd").and_then(Value::as_str).map(PathBuf::from)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

/// The limit message the session stopped on, if it stopped on one.
pub fn limit_message(session: &Session) -> Option<String> {
    let tail = read_tail(&session.path, TAIL_BYTES).ok()?;
    let lines = tail.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok());
    match session.provider {
        Provider::Claude => claude_limit(lines),
        Provider::Codex => codex_limit(lines),
    }
}

/// The last assistant entry decides: an error entry after a reset and a
/// successful turn is history, not the current state.
fn claude_limit(lines: impl Iterator<Item = Value>) -> Option<String> {
    let last = lines.filter(|line| line["type"] == "assistant").last()?;
    if last["isApiErrorMessage"] != true || last["error"] != "rate_limit" {
        return None;
    }
    let text = last
        .pointer("/message/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or("usage limit reached");
    Some(text.to_string())
}

/// The last finished turn decides, as for Claude.
fn codex_limit(lines: impl Iterator<Item = Value>) -> Option<String> {
    let last = lines
        .filter(|line| line["type"] == "event_msg" && line["payload"]["type"] == "task_complete")
        .last()?;
    let error = &last["payload"]["error"];
    if error["codex_error_info"] != "usage_limit_exceeded" {
        return None;
    }
    Some(error["message"].as_str().unwrap_or("usage limit reached").to_string())
}

/// The last `max` bytes of a file, from the first whole line on.
fn read_tail(path: &Path, max: u64) -> io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(match (start, text.find('\n')) {
        (0, _) => text.into_owned(),
        (_, Some(newline)) => text[newline + 1..].to_string(),
        (_, None) => String::new(),
    })
}

/// Copy the transcript to the same place under `to`'s CLI home, where that
/// account's CLI looks it up by id. An older copy there is replaced.
pub fn copy_to(session: &Session, to: &Account) -> io::Result<PathBuf> {
    let home = to.cli_home().ok_or_else(|| io::Error::other(format!("{to} has no home directory")))?;
    let dest = home.join(&session.relative);
    if dest == session.path {
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&session.path, &dest)?;
    Ok(dest)
}

/// The CLI arguments that resume `session` and send `prompt` as the next message.
pub fn resume_args(session: &Session, prompt: &str) -> Vec<String> {
    let id = session.id.clone();
    match session.provider {
        Provider::Claude => vec!["--resume".into(), id, prompt.into()],
        Provider::Codex => vec!["resume".into(), id, prompt.into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_lines(path: &Path, lines: &[Value]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(path, body).unwrap();
    }

    fn claude_error(error: &str, text: &str) -> Value {
        json!({ "type": "assistant", "isApiErrorMessage": true, "error": error,
                "message": { "role": "assistant", "content": [{ "type": "text", "text": text }] } })
    }

    fn claude_reply(text: &str) -> Value {
        json!({ "type": "assistant", "message": { "role": "assistant", "content": [{ "type": "text", "text": text }] } })
    }

    fn codex_turn(error: Option<Value>) -> Value {
        json!({ "type": "event_msg", "payload": { "type": "task_complete", "error": error } })
    }

    fn account(provider: Provider, home: &Path) -> Account {
        Account { id: "p".into(), provider, label: "p".into(), home: Some(home.to_path_buf()) }
    }

    #[test]
    fn a_claude_session_that_ended_on_a_rate_limit_is_a_limit_hit() {
        let lines = vec![
            json!({ "type": "user", "message": { "role": "user", "content": "go" } }),
            claude_error("rate_limit", "You've hit your session limit · resets 3:10am"),
            json!({ "type": "system" }),
        ];
        assert_eq!(
            claude_limit(lines.into_iter()),
            Some("You've hit your session limit · resets 3:10am".into())
        );
    }

    #[test]
    fn a_later_reply_or_another_error_is_not_a_limit_hit() {
        let recovered = vec![claude_error("rate_limit", "limit"), claude_reply("done")];
        assert_eq!(claude_limit(recovered.into_iter()), None);
        let offline = vec![claude_error("server_error", "API Error: 529 Overloaded")];
        assert_eq!(claude_limit(offline.into_iter()), None);
    }

    #[test]
    fn a_codex_turn_that_ended_on_the_usage_limit_is_a_limit_hit() {
        let hit = codex_turn(Some(json!({
            "message": "You've hit your usage limit.", "codex_error_info": "usage_limit_exceeded"
        })));
        assert_eq!(codex_limit(vec![codex_turn(None), hit.clone()].into_iter()), Some("You've hit your usage limit.".into()));
        assert_eq!(codex_limit(vec![hit, codex_turn(None)].into_iter()), None);
    }

    #[test]
    fn the_latest_claude_session_is_found_copied_and_resumable_elsewhere() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let cwd = Path::new("/work/my repo");
        let since = SystemTime::now() - std::time::Duration::from_secs(5);
        let file = from.path().join("projects/-work-my-repo/0b6f-id.jsonl");
        write_lines(&file, &[claude_error("rate_limit", "limit")]);

        let found = latest(&account(Provider::Claude, from.path()), cwd, since).unwrap();
        assert_eq!(found.id, "0b6f-id");
        assert_eq!(limit_message(&found), Some("limit".into()));

        let copy = copy_to(&found, &account(Provider::Claude, to.path())).unwrap();
        assert_eq!(copy, to.path().join("projects/-work-my-repo/0b6f-id.jsonl"));
        assert_eq!(resume_args(&found, "go on"), ["--resume", "0b6f-id", "go on"]);

        let later = SystemTime::now() + std::time::Duration::from_secs(60);
        assert_eq!(latest(&account(Provider::Claude, from.path()), cwd, later), None);
    }

    #[test]
    fn the_latest_codex_session_is_the_one_for_this_folder() {
        let home = tempfile::tempdir().unwrap();
        let since = SystemTime::now() - std::time::Duration::from_secs(5);
        let id = "01a0e2f4-89ff-72f1-9090-05252f373ba2";
        let mine = home.path().join(format!("sessions/2026/09/27/rollout-2026-09-27T22-01-12-{id}.jsonl"));
        let other = home.path().join("sessions/2026/09/28/rollout-2026-09-28T01-00-00-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl");
        write_lines(&mine, &[json!({ "type": "session_meta", "payload": { "cwd": "/work/a" } })]);
        write_lines(&other, &[json!({ "type": "session_meta", "payload": { "cwd": "/work/b" } })]);

        let found = latest(&account(Provider::Codex, home.path()), Path::new("/work/a"), since).unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.relative, PathBuf::from(format!("sessions/2026/09/27/rollout-2026-09-27T22-01-12-{id}.jsonl")));
        assert_eq!(resume_args(&found, "go on"), ["resume", id, "go on"]);
    }

    #[test]
    fn the_tail_starts_at_a_whole_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, "first line\nsecond\nthird\n").unwrap();
        assert_eq!(read_tail(&path, 13).unwrap(), "third\n");
        assert_eq!(read_tail(&path, 1024).unwrap(), "first line\nsecond\nthird\n");
    }
}
