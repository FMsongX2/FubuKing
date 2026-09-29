//! Agent session transcripts: where each CLI writes them, whether one stopped
//! on a usage limit, and copying one to another account so it resumes there.
//!
//! Both CLIs record a limit hit in the transcript itself. Claude Code writes
//! an assistant entry with `isApiErrorMessage: true` and `error: "rate_limit"`;
//! Codex ends the turn with a `task_complete` event whose
//! `error.codex_error_info` is `usage_limit_exceeded`. Neither format is a
//! documented interface, so when one changes a limit goes unnoticed: the run
//! simply ends, it never hands off by mistake. What such a change looks like
//! is reported instead: a session that ends on a limit in another form
//! ([`Ending::Unread`]), or one saved where FubuKing does not look ([`stray`]).
//!
//! Resuming a copy works because each CLI looks a session up by id under its
//! own home: `<CLAUDE_CONFIG_DIR>/projects/<encoded cwd>/<id>.jsonl` and
//! `<CODEX_HOME>/sessions/<yyyy>/<mm>/<dd>/rollout-<time>-<id>.jsonl`. Checked
//! against Claude Code 2.1.273 and Codex 0.147.0; neither documents it.
//! `tests/handoff` replays real records of later versions against all of it.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use atlas_agent_transcript::{is_injected_user_text, strip_injected_context};
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
    let path = codex_rollouts(home, since)
        .into_iter()
        .find(|path| codex_cwd(path).is_some_and(|recorded| same_dir(&recorded, cwd)))?;
    session(Provider::Codex, home, path, codex_id)
}

/// The rollouts under `home` modified at or after `since`, newest first.
fn codex_rollouts(home: &Path, since: SystemTime) -> Vec<PathBuf> {
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
    candidates.into_iter().map(|(path, _)| path).collect()
}

/// `rollout-2026-09-27T22-01-12-<uuid>`: the id is the last five groups.
fn codex_id(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let parts: Vec<&str> = stem.rsplitn(6, '-').collect();
    (parts.len() == 6).then(|| parts[..5].iter().rev().copied().collect::<Vec<_>>().join("-"))
}

/// A session `account`'s CLI saved since `since` that [`latest`] does not
/// find for `cwd`: where one goes when the CLI's layout changes. For Claude,
/// a transcript under another folder of `projects/` whose entries record
/// `cwd`; for Codex, the newest rollout when FubuKing cannot read its header,
/// or cannot name one it can read for `cwd`.
pub fn stray(account: &Account, cwd: &Path, since: SystemTime) -> Option<PathBuf> {
    let home = account.cli_home()?;
    match account.provider {
        Provider::Claude => {
            let projects = home.join("projects");
            let expected = projects.join(atlas_agent_transcript::encode_cwd(&cwd.to_string_lossy()));
            std::fs::read_dir(&projects)
                .ok()?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|dir| dir.is_dir() && *dir != expected)
                .flat_map(|dir| jsonl_files(&dir).filter(|(_, modified)| *modified >= since).map(|(path, _)| path))
                .find(|path| records_cwd(path, cwd))
        }
        Provider::Codex => {
            let newest = codex_rollouts(&home, since).into_iter().next()?;
            match codex_cwd(&newest) {
                Some(recorded) if !same_dir(&recorded, cwd) => None,
                _ => Some(newest),
            }
        }
    }
}

/// Whether one of a Claude transcript's first entries records `cwd`.
fn records_cwd(path: &Path, cwd: &Path) -> bool {
    let Ok(file) = File::open(path) else { return false };
    BufReader::new(file)
        .lines()
        .take(20)
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .any(|entry| entry["cwd"].as_str().is_some_and(|recorded| same_dir(Path::new(recorded), cwd)))
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

/// How a session's last turn ended, read the way a handoff reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// A usage limit, in the CLI's words: the account's, or a model's (see
    /// [`is_account_limit`]).
    Limit(String),
    /// A limit in a form FubuKing does not read as one: an error about a
    /// limit other than the known one, or a limit's words outside an error.
    /// What a changed transcript format looks like. What the CLI said.
    Unread(String),
    /// Anything else.
    Other,
}

/// How `session` ended, if it ended at or after `since`. An ending written
/// before the run started belongs to an earlier run of that session: the one
/// a resumed copy carries over from the previous account, or one a later run
/// touched the file after. The CLI stamps entries from the same clock, so no
/// slack is needed.
pub fn ending(session: &Session, since: SystemTime) -> Ending {
    let Ok(tail) = read_tail(&session.path, TAIL_BYTES) else { return Ending::Other };
    let lines = tail.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok());
    let found = match session.provider {
        Provider::Claude => claude_ending(lines),
        Provider::Codex => codex_ending(lines),
    };
    let Some((entry, ending)) = found else { return Ending::Other };
    let stamped = entry["timestamp"]
        .as_str()
        .and_then(|stamp| chrono::DateTime::parse_from_rfc3339(stamp).ok())
        .map(SystemTime::from);
    match stamped {
        Some(at) if at < since => Ending::Other,
        _ => ending,
    }
}

/// The limit message the session stopped on, if it stopped on one at or after
/// `since`.
pub fn limit_message(session: &Session, since: SystemTime) -> Option<String> {
    match ending(session, since) {
        Ending::Limit(message) => Some(message),
        _ => None,
    }
}

/// The last assistant entry decides: an error entry after a reset and a
/// successful turn is history, not the current state.
fn claude_ending(lines: impl Iterator<Item = Value>) -> Option<(Value, Ending)> {
    let last = lines.filter(|line| line["type"] == "assistant").last()?;
    let text = last.pointer("/message/content/0/text").and_then(Value::as_str);
    let error = label(&last["error"]);
    let ending = if last["isApiErrorMessage"] == true && error == "rate_limit" {
        Ending::Limit(text.unwrap_or("usage limit reached").to_string())
    } else if last["isApiErrorMessage"] == true && (error.contains("limit") || text.is_some_and(reads_like_limit)) {
        Ending::Unread(format!("{error}: {}", text.unwrap_or_default()))
    } else if text.is_some_and(reads_like_limit) {
        Ending::Unread(text.unwrap_or_default().to_string())
    } else {
        Ending::Other
    };
    Some((last, ending))
}

/// The last finished turn decides, as for Claude.
fn codex_ending(lines: impl Iterator<Item = Value>) -> Option<(Value, Ending)> {
    let last = lines
        .filter(|line| line["type"] == "event_msg" && line["payload"]["type"] == "task_complete")
        .last()?;
    let error = &last["payload"]["error"];
    let message = error["message"].as_str();
    let info = label(&error["codex_error_info"]);
    let reply = last["payload"]["last_agent_message"].as_str();
    let ending = if error.is_object() && info == "usage_limit_exceeded" {
        Ending::Limit(message.unwrap_or("usage limit reached").to_string())
    } else if error.is_object() && (info.contains("limit") || message.is_some_and(reads_like_limit)) {
        Ending::Unread(format!("{info}: {}", message.unwrap_or_default()))
    } else if reply.is_some_and(reads_like_limit) {
        Ending::Unread(reply.unwrap_or_default().to_string())
    } else {
        Ending::Other
    };
    Some((last, ending))
}

/// An error kind as text: a string, or the JSON of anything else.
fn label(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "error".to_string(),
        other => other.to_string(),
    }
}

/// The words both CLIs use when an account runs out: "You've hit your session
/// limit", "You've hit your usage limit".
fn reads_like_limit(text: &str) -> bool {
    let text = text.to_lowercase();
    text.contains("limit") && (text.contains("hit your") || text.contains("reached your"))
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

/// Whether a limit message means the whole account is out, rather than one
/// model: Claude's model limits say to switch to another model, which the
/// same account can still do.
pub fn is_account_limit(provider: Provider, message: &str) -> bool {
    match provider {
        Provider::Claude => !message.contains("another model"),
        Provider::Codex => true,
    }
}

/// How many of the session's requests and replies a brief quotes, and how
/// long each may be.
const BRIEF_REQUESTS: usize = 8;
const BRIEF_REPLIES: usize = 2;
const REQUEST_CHARS: usize = 1_500;
const REPLY_CHARS: usize = 2_500;

/// What the user asked in a session and what the agent last answered.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Conversation {
    /// The first request, then the latest ones.
    pub requests: Vec<String>,
    /// Requests between the first and the latest that were left out.
    pub skipped: usize,
    /// The latest replies, oldest first.
    pub replies: Vec<String>,
}

/// Read a session start to end, keeping only what a brief quotes.
pub fn conversation(session: &Session) -> io::Result<Conversation> {
    let reader = BufReader::new(File::open(&session.path)?);
    let mut first: Option<String> = None;
    let mut latest: VecDeque<String> = VecDeque::new();
    let mut replies: VecDeque<String> = VecDeque::new();
    let mut total = 0usize;
    for line in reader.lines() {
        let Ok(value) = serde_json::from_str::<Value>(&line?) else { continue };
        let turn = match session.provider {
            Provider::Claude => claude_turn(&value),
            Provider::Codex => codex_turn(&value),
        };
        match turn {
            Some((true, text)) => {
                total += 1;
                if first.is_none() {
                    first = Some(text);
                } else {
                    latest.push_back(text);
                    if latest.len() > BRIEF_REQUESTS - 1 {
                        latest.pop_front();
                    }
                }
            }
            Some((false, text)) => {
                replies.push_back(text);
                if replies.len() > BRIEF_REPLIES {
                    replies.pop_front();
                }
            }
            None => {}
        }
    }
    let requests: Vec<String> = first.into_iter().chain(latest).collect();
    Ok(Conversation { skipped: total.saturating_sub(requests.len()), requests, replies: replies.into() })
}

/// `(is the user, text)` for a Claude Code entry that is a request or a reply.
fn claude_turn(line: &Value) -> Option<(bool, String)> {
    if line["isSidechain"] == true || line["isMeta"] == true || line["isApiErrorMessage"] == true {
        return None;
    }
    let user = match line["type"].as_str()? {
        "user" => true,
        "assistant" => false,
        _ => return None,
    };
    let text = match &line["message"]["content"] {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = if user { strip_injected_context(&text) } else { text };
    let text = text.trim();
    if text.is_empty() || (user && is_injected_user_text(text)) {
        return None;
    }
    Some((user, text.to_string()))
}

/// The same for a Codex rollout line. Codex sends its instructions and
/// environment as user messages too; those are left out.
fn codex_turn(line: &Value) -> Option<(bool, String)> {
    let payload = &line["payload"];
    if line["type"] != "response_item" || payload["type"] != "message" {
        return None;
    }
    let user = match payload["role"].as_str()? {
        "user" => true,
        "assistant" => false,
        _ => return None,
    };
    let text = payload["content"]
        .as_array()?
        .iter()
        .filter_map(|item| item["text"].as_str())
        .map(str::trim)
        .filter(|text| !user || !(is_injected_user_text(text) || text.starts_with("# AGENTS.md instructions")))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some((user, text))
}

/// A prompt that lets another agent, or another session of the same one,
/// take over `session`: what was asked, the last answers, and where to look.
pub fn brief(session: &Session, from: &Account) -> io::Result<String> {
    let conversation = conversation(session)?;
    let agent = match session.provider {
        Provider::Claude => "Claude Code",
        Provider::Codex => "Codex",
    };
    let mut out = format!(
        "You are taking over a coding task in this repository from another session. It ran in {agent} \
         ({from}) and stopped because that account hit its usage limit.\n\nThe user's requests in that \
         session, oldest first:\n"
    );
    for (index, request) in conversation.requests.iter().enumerate() {
        if index == 1 && conversation.skipped > 0 {
            out.push_str(&format!("(… {} more requests)\n", conversation.skipped));
        }
        out.push_str(&format!("{}. {}\n", index + 1, clip(request, REQUEST_CHARS)));
    }
    if !conversation.replies.is_empty() {
        out.push_str("\nIts last replies:\n");
        for reply in &conversation.replies {
            out.push_str(&format!("---\n{}\n", clip(reply, REPLY_CHARS)));
        }
        out.push_str("---\n");
    }
    out.push_str(
        "\nBefore changing anything, run `git status` and `git diff` to see the work in progress, and call \
         memory_briefing for what earlier sessions recorded. Then continue the task from where it stopped.",
    );
    Ok(out)
}

/// At most `max` characters, marked when cut.
fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{} […]", &text[..cut]),
        None => text.to_string(),
    }
}

/// Copy the transcript to the same place under `to`'s CLI home, where that
/// account's CLI looks it up by id. An older copy there that this one extends
/// is replaced; one that went its own way (the session carried on under `to`
/// meanwhile) is kept beside it as a `.bak` first.
pub fn copy_to(session: &Session, to: &Account) -> io::Result<PathBuf> {
    let home = to.cli_home().ok_or_else(|| io::Error::other(format!("{to} has no home directory")))?;
    let dest = home.join(&session.relative);
    if dest == session.path {
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if dest.exists() && !std::fs::read(&session.path)?.starts_with(&std::fs::read(&dest)?) {
        let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default();
        let mut backup = dest.clone().into_os_string();
        backup.push(format!(".{stamp}.bak"));
        std::fs::rename(&dest, backup)?;
    }
    std::fs::copy(&session.path, &dest)?;
    Ok(dest)
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

    fn claude_end(lines: Vec<Value>) -> Ending {
        claude_ending(lines.into_iter()).map_or(Ending::Other, |(_, ending)| ending)
    }

    fn codex_end(lines: Vec<Value>) -> Ending {
        codex_ending(lines.into_iter()).map_or(Ending::Other, |(_, ending)| ending)
    }

    #[test]
    fn a_claude_session_that_ended_on_a_rate_limit_is_a_limit_hit() {
        let lines = vec![
            json!({ "type": "user", "message": { "role": "user", "content": "go" } }),
            claude_error("rate_limit", "You've hit your session limit · resets 3:10am"),
            json!({ "type": "system" }),
        ];
        assert_eq!(claude_end(lines), Ending::Limit("You've hit your session limit · resets 3:10am".into()));
    }

    #[test]
    fn a_later_reply_or_another_error_is_not_a_limit_hit() {
        let recovered = vec![claude_error("rate_limit", "limit"), claude_reply("done")];
        assert_eq!(claude_end(recovered), Ending::Other);
        let offline = vec![claude_error("server_error", "API Error: 529 Overloaded")];
        assert_eq!(claude_end(offline), Ending::Other);
    }

    #[test]
    fn a_codex_turn_that_ended_on_the_usage_limit_is_a_limit_hit() {
        let hit = codex_turn(Some(json!({
            "message": "You've hit your usage limit.", "codex_error_info": "usage_limit_exceeded"
        })));
        assert_eq!(codex_end(vec![codex_turn(None), hit.clone()]), Ending::Limit("You've hit your usage limit.".into()));
        assert_eq!(codex_end(vec![hit, codex_turn(None)]), Ending::Other);
    }

    /// What a format change looks like: the limit is there, in another form.
    #[test]
    fn a_limit_in_another_form_is_reported_not_taken() {
        assert_eq!(
            claude_end(vec![claude_error("rate_limit_exceeded", "You've hit your weekly limit")]),
            Ending::Unread("rate_limit_exceeded: You've hit your weekly limit".into())
        );
        assert_eq!(
            claude_end(vec![claude_reply("You've hit your session limit · resets 3:10am")]),
            Ending::Unread("You've hit your session limit · resets 3:10am".into())
        );
        let renamed = codex_turn(Some(json!({ "message": "You've hit your usage limit.", "codex_error_info": "quota_exceeded" })));
        assert_eq!(codex_end(vec![renamed]), Ending::Unread("quota_exceeded: You've hit your usage limit.".into()));
        let busy = codex_turn(Some(json!({ "message": "stream disconnected", "codex_error_info": { "response_stream_disconnected": {} } })));
        assert_eq!(codex_end(vec![busy]), Ending::Other);
    }

    #[test]
    fn a_claude_session_saved_under_another_folder_is_found_as_a_stray() {
        let home = tempfile::tempdir().unwrap();
        let cwd = Path::new("/work/my repo");
        let since = SystemTime::now() - std::time::Duration::from_secs(5);
        let theirs = home.path().join("projects/-work-other/a.jsonl");
        write_lines(&theirs, &[json!({ "type": "user", "cwd": "/work/other" })]);
        assert_eq!(stray(&account(Provider::Claude, home.path()), cwd, since), None);
        let moved = home.path().join("projects/work_my_repo/b.jsonl");
        write_lines(&moved, &[json!({ "type": "user", "cwd": "/work/my repo" })]);
        assert_eq!(latest(&account(Provider::Claude, home.path()), cwd, since), None);
        assert_eq!(stray(&account(Provider::Claude, home.path()), cwd, since), Some(moved));
    }

    #[test]
    fn a_codex_rollout_with_a_header_fubuking_cannot_read_is_a_stray() {
        let home = tempfile::tempdir().unwrap();
        let since = SystemTime::now() - std::time::Duration::from_secs(5);
        let other = home.path().join("sessions/2026/09/28/rollout-2026-09-28T01-00-00-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl");
        write_lines(&other, &[json!({ "type": "session_meta", "payload": { "cwd": "/work/b" } })]);
        assert_eq!(stray(&account(Provider::Codex, home.path()), Path::new("/work/a"), since), None);
        let unreadable = home.path().join("sessions/2026/09/29/rollout-2026-09-29T01-00-00-aaaaaaaa-bbbb-cccc-dddd-ffffffffffff.jsonl");
        write_lines(&unreadable, &[json!({ "type": "thread_meta", "thread": { "cwd": "/work/a" } })]);
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
        std::fs::File::options().append(true).open(&unreadable).unwrap().set_modified(later).unwrap();
        assert_eq!(stray(&account(Provider::Codex, home.path()), Path::new("/work/a"), since), Some(unreadable));
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
        assert_eq!(limit_message(&found, since), Some("limit".into()));

        let copy = copy_to(&found, &account(Provider::Claude, to.path())).unwrap();
        assert_eq!(copy, to.path().join("projects/-work-my-repo/0b6f-id.jsonl"));

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
    }

    #[test]
    fn a_copy_that_went_its_own_way_is_kept_as_a_backup() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let rel = PathBuf::from("projects/-w/s.jsonl");
        std::fs::create_dir_all(from.path().join("projects/-w")).unwrap();
        std::fs::create_dir_all(to.path().join("projects/-w")).unwrap();
        std::fs::write(from.path().join(&rel), "a\nb\n").unwrap();
        let session = Session { provider: Provider::Claude, id: "s".into(), path: from.path().join(&rel), relative: rel.clone() };

        // A copy the source extends is simply replaced.
        std::fs::write(to.path().join(&rel), "a\n").unwrap();
        copy_to(&session, &account(Provider::Claude, to.path())).unwrap();
        assert_eq!(std::fs::read_dir(to.path().join("projects/-w")).unwrap().count(), 1);

        // One with turns of its own is kept.
        std::fs::write(to.path().join(&rel), "a\nc\n").unwrap();
        copy_to(&session, &account(Provider::Claude, to.path())).unwrap();
        assert_eq!(std::fs::read_to_string(to.path().join(&rel)).unwrap(), "a\nb\n");
        let kept: Vec<String> = std::fs::read_dir(to.path().join("projects/-w"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".bak"))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read_to_string(to.path().join("projects/-w").join(&kept[0])).unwrap(), "a\nc\n");
    }

    #[test]
    fn a_limit_stamped_before_the_run_is_an_earlier_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects/-w/s.jsonl");
        let mut old = claude_error("rate_limit", "old limit");
        old["timestamp"] = json!("2026-09-01T10:00:00.000Z");
        write_lines(&path, &[old]);
        let session = Session { provider: Provider::Claude, id: "s".into(), path: path.clone(), relative: PathBuf::new() };
        let started = SystemTime::now();
        assert_eq!(limit_message(&session, started), None);

        let mut fresh = claude_error("rate_limit", "new limit");
        fresh["timestamp"] = json!(chrono::Utc::now().to_rfc3339());
        write_lines(&path, &[fresh]);
        assert_eq!(limit_message(&session, started), Some("new limit".into()));
    }

    #[test]
    fn only_a_model_limit_leaves_the_account_usable() {
        assert!(is_account_limit(Provider::Claude, "You've hit your session limit · resets 3:10am"));
        assert!(!is_account_limit(
            Provider::Claude,
            "You've reached your Fable limit. Switch to another model, or manage usage credits"
        ));
        assert!(is_account_limit(Provider::Codex, "You've hit your usage limit."));
    }

    #[test]
    fn a_claude_brief_quotes_requests_and_the_last_reply_without_injected_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects/-w/s.jsonl");
        let mut lines = vec![
            json!({ "type": "user", "message": { "role": "user", "content": "<command-name>/clear</command-name>" } }),
            json!({ "type": "user", "message": { "role": "user", "content": "Port the parser to Rust" } }),
            json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "content": "ok" }] } }),
            claude_reply("Parser ported; tests next."),
        ];
        for n in 0..10 {
            lines.push(json!({ "type": "user", "message": { "role": "user", "content": format!("step {n}") } }));
        }
        lines.push(claude_error("rate_limit", "You've hit your session limit"));
        write_lines(&path, &lines);
        let session = Session { provider: Provider::Claude, id: "s".into(), path, relative: PathBuf::new() };

        let talk = conversation(&session).unwrap();
        assert_eq!(talk.requests.first().map(String::as_str), Some("Port the parser to Rust"));
        assert_eq!(talk.requests.last().map(String::as_str), Some("step 9"));
        assert_eq!(talk.requests.len(), BRIEF_REQUESTS);
        assert_eq!(talk.skipped, 11 - BRIEF_REQUESTS);
        assert_eq!(talk.replies, ["Parser ported; tests next."]);

        let brief = brief(&session, &account(Provider::Claude, dir.path())).unwrap();
        assert!(brief.contains("ran in Claude Code (claude account `p`)"));
        assert!(brief.contains("1. Port the parser to Rust"));
        assert!(brief.contains("(… 3 more requests)"));
        assert!(!brief.contains("/clear") && !brief.contains("session limit"));
    }

    #[test]
    fn a_codex_brief_leaves_out_instructions_and_environment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions/r.jsonl");
        let message = |role: &str, kind: &str, texts: &[&str]| {
            json!({ "type": "response_item", "payload": { "type": "message", "role": role,
                "content": texts.iter().map(|t| json!({ "type": kind, "text": t })).collect::<Vec<_>>() } })
        };
        write_lines(&path, &[
            message("developer", "input_text", &["system rules"]),
            message("user", "input_text", &["# AGENTS.md instructions\n<INSTRUCTIONS>…", "<environment_context>…</environment_context>"]),
            message("user", "input_text", &["Add a retry to the uploader"]),
            message("assistant", "output_text", &["Retry added with backoff."]),
        ]);
        let session = Session { provider: Provider::Codex, id: "r".into(), path, relative: PathBuf::new() };
        let talk = conversation(&session).unwrap();
        assert_eq!(talk.requests, ["Add a retry to the uploader"]);
        assert_eq!(talk.replies, ["Retry added with backoff."]);
    }

    #[test]
    fn clipping_marks_the_cut_and_respects_characters() {
        assert_eq!(clip("한글 문장입니다", 4), "한글 문 […]");
        assert_eq!(clip("short", 10), "short");
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
