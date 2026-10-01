//! The fake `claude` and `codex`. Each does what FubuKing relies on in the
//! real CLI and nothing more: it answers `--help` with the real help, and it
//! writes the session it is given where the real CLI keeps it, in its format,
//! built from the records in `fixtures/`.
//!
//! An account's CLI home says how its runs end: a `fake-plan` file reading
//! `limit` stops them on the usage limit, `lost` makes a resume find no
//! session, and without one they finish. Two plans play a CLI release that
//! changed what FubuKing reads: `renamed` stops on a limit whose entry names
//! its error differently, `moved` keeps the session in another folder. After
//! a limit an interactive run stays open, as the real CLIs do, until it is
//! stopped. `--version` answers the newest version FubuKing is tested with,
//! or the one in `<state>/<program>-version`.

use std::fs::OpenOptions;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fubuking::accounts::Provider;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The variable naming the directory the fakes record their runs in.
pub const STATE: &str = "FUBUKING_FAKE_STATE";
/// The file in a CLI home that says how the account's runs end.
pub const PLAN: &str = "fake-plan";
pub const LIMIT: &str = "limit";
pub const LOST: &str = "lost";
pub const RENAMED: &str = "renamed";
pub const MOVED: &str = "moved";

/// The reply a new session gets before anything else happens, and the one a
/// session ends with when it finishes.
pub const WORKING: &str = "Found the race: the retry loop reads the counter before it takes the lock.";
pub const DONE: &str = "Done: the lock covers the read now, and the test passes 50 runs in a row.";

/// One run of a fake CLI, other than `--help` and Codex's app server.
#[derive(Debug, Serialize, Deserialize)]
pub struct Call {
    pub program: String,
    /// The CLI home the run used: a profile's, or the default login's.
    pub home: PathBuf,
    pub args: Vec<String>,
}

const CLAUDE_HELP: &str = include_str!("../fixtures/claude-2.1.283-help.txt");
const CODEX_HELP: &str = include_str!("../fixtures/codex-0.157.1-help.txt");
const CODEX_EXEC_HELP: &str = include_str!("../fixtures/codex-0.157.1-exec-help.txt");
const CLAUDE_RECORDS: &str = include_str!("../fixtures/claude.json");
const CODEX_RECORDS: &str = include_str!("../fixtures/codex.json");

/// The options of each CLI that take the next word, and Claude's that take
/// every word up to the next option, as far as the tests and FubuKing use them.
const CLAUDE_ONE: &[&str] = &["--model", "--settings", "-r", "--resume", "--session-id", "--permission-mode"];
const CLAUDE_MANY: &[&str] = &["--allowedTools", "--allowed-tools", "--mcp-config", "--add-dir"];
const CODEX_ONE: &[&str] = &["-c", "--config", "-m", "--model"];

/// `claude`: `-p` or interactive, a new session or `--resume <id>`.
pub fn claude() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|word| word == "--help" || word == "-h") {
        print!("{CLAUDE_HELP}");
        return 0;
    }
    if args.iter().any(|word| word == "--version" || word == "-v") {
        println!("{} (Claude Code)", version("claude", Provider::Claude));
        return 0;
    }
    let home = home_of("CLAUDE_CONFIG_DIR", ".claude");
    record("claude", &home, &args);
    let options = &args[..args.iter().position(|word| word == "--").unwrap_or(args.len())];
    let print = options.iter().any(|word| word == "-p" || word == "--print");
    let resume = options
        .iter()
        .position(|word| word == "-r" || word == "--resume")
        .and_then(|at| options.get(at + 1))
        .cloned();
    let prompt = positionals(&args, CLAUDE_ONE, CLAUDE_MANY).join(" ");
    let cwd = cwd();
    let plan = plan(&home);
    let folder = if plan == MOVED { format!("moved{}", encode(&cwd)) } else { encode(&cwd) };
    let dir = home.join("projects").join(folder);
    let fresh = resume.is_none();
    let id = match resume {
        Some(id) if plan != LOST && dir.join(format!("{id}.jsonl")).exists() => id,
        Some(id) => {
            eprintln!("No conversation found with session ID: {id}");
            return 1;
        }
        // No message, no session: what `claude` alone, as in a login, leaves.
        None if prompt.is_empty() => return 0,
        None => next_id(),
    };
    let mut session = ClaudeSession::new(dir.join(format!("{id}.jsonl")), id, cwd);
    session.write("request", &prompt);
    if fresh {
        session.write("reply", WORKING);
    }
    if plan == LIMIT || plan == RENAMED {
        session.limit(if plan == RENAMED { "rate_limit_exceeded" } else { "rate_limit" });
        return if print { 1 } else { wait_to_be_stopped() };
    }
    session.write("reply", DONE);
    0
}

/// `codex`: `exec` or interactive, a new session or `resume <id>`, `login`,
/// and an app server with no rate limits to report.
pub fn codex() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words = positionals(&args, CODEX_ONE, &[]);
    let subcommand = words.first().map(String::as_str);
    if args.iter().any(|word| word == "--help" || word == "-h") {
        print!("{}", if subcommand == Some("exec") { CODEX_EXEC_HELP } else { CODEX_HELP });
        return 0;
    }
    if args.iter().any(|word| word == "--version" || word == "-V") {
        println!("codex-cli {}", version("codex", Provider::Codex));
        return 0;
    }
    // FubuKing reads each account's room from the app server; with nothing
    // read, the room is unknown and the account still counts.
    if subcommand == Some("app-server") {
        let home = home_of("CODEX_HOME", ".codex");
        return if plan(&home) == "quota" { quota_server() } else { 1 };
    }
    let home = home_of("CODEX_HOME", ".codex");
    record("codex", &home, &args);
    let (batch, words) = match subcommand {
        Some("login") => return 0,
        Some("exec" | "e") => (true, &words[1..]),
        _ => (false, &words[..]),
    };
    let (resume, prompt) = match words.first().map(String::as_str) {
        Some("resume") => (words.get(1).cloned(), words.get(2..).unwrap_or_default().join(" ")),
        _ => (None, words.join(" ")),
    };
    let cwd = cwd();
    let mut session = match resume {
        Some(id) => match rollout(&home.join("sessions"), &id).filter(|_| plan(&home) != LOST) {
            Some(path) => CodexSession::open(path, cwd),
            None => {
                eprintln!("Error: no rollout found for thread id {id}");
                return 1;
            }
        },
        None if prompt.is_empty() => return 0,
        None => CodexSession::start(&home, next_id(), cwd, batch),
    };
    let fresh = session.fresh;
    session.write("request", &prompt);
    if fresh {
        session.write("reply", WORKING);
    }
    if plan(&home) == LIMIT {
        session.write("limit", "");
        return if batch { 1 } else { wait_to_be_stopped() };
    }
    session.write("reply", DONE);
    session.write("complete", "");
    0
}

fn quota_server() -> i32 {
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = request.get("id") else { continue };
        let result = match request.get("method").and_then(Value::as_str) {
            Some("account/read") => serde_json::json!({ "account": { "type": "chatgpt", "planType": "plus" } }),
            Some("account/rateLimits/read") => serde_json::json!({
                "rateLimits": { "primary": { "usedPercent": 20.0, "windowDurationMins": 300 } }
            }),
            _ => serde_json::json!({}),
        };
        println!("{}", serde_json::json!({ "id": id, "result": result }));
    }
    0
}

/// A Claude Code transcript: each entry chained to the one before it.
struct ClaudeSession {
    path: PathBuf,
    id: String,
    cwd: String,
    records: Value,
    last: Option<String>,
}

impl ClaudeSession {
    fn new(path: PathBuf, id: String, cwd: String) -> Self {
        let records = serde_json::from_str(CLAUDE_RECORDS).expect("claude.json");
        Self { path, id, cwd, records, last: None }
    }

    /// Append the `request` or `reply` entry with `text` as its words.
    fn write(&mut self, kind: &str, text: &str) {
        let mut entry = self.records[kind].clone();
        match kind {
            "request" => entry["message"]["content"] = text.into(),
            _ => entry["message"]["content"][0]["text"] = text.into(),
        }
        self.put(entry);
    }

    /// Append the limit entry, its `error` named `error`.
    fn limit(&mut self, error: &str) {
        let mut entry = self.records["limit"].clone();
        entry["error"] = error.into();
        self.put(entry);
    }

    fn put(&mut self, mut entry: Value) {
        let uuid = next_id();
        entry["parentUuid"] = self.last.replace(uuid.clone()).into();
        entry["uuid"] = uuid.into();
        entry["sessionId"] = self.id.clone().into();
        entry["cwd"] = self.cwd.clone().into();
        entry["timestamp"] = now().into();
        append(&self.path, &entry);
    }
}

/// A Codex rollout: a header, then numbered lines.
struct CodexSession {
    path: PathBuf,
    cwd: String,
    records: Value,
    ordinal: usize,
    fresh: bool,
}

impl CodexSession {
    /// A new rollout under `home`, named and filed by local time as Codex does,
    /// with its header and the instructions Codex sends as a user message.
    fn start(home: &Path, id: String, cwd: String, batch: bool) -> Self {
        let local = chrono::Local::now();
        let path = home
            .join("sessions")
            .join(local.format("%Y/%m/%d").to_string())
            .join(format!("rollout-{}-{id}.jsonl", local.format("%Y-%m-%dT%H-%M-%S")));
        let mut session = Self { path, cwd, records: codex_records(), ordinal: 0, fresh: true };
        let mut meta = session.records["session_meta"].clone();
        let payload = &mut meta["payload"];
        for key in ["id", "session_id"] {
            payload[key] = id.clone().into();
        }
        payload["cwd"] = session.cwd.clone().into();
        payload["runtime_workspace_roots"] = vec![session.cwd.clone()].into();
        payload["timestamp"] = now().into();
        if !batch {
            // What an interactive Codex 0.157.1 session records.
            payload["originator"] = "codex-tui".into();
            payload["source"] = "cli".into();
        }
        session.put(meta);
        session.write("instructions", "");
        session
    }

    fn open(path: PathBuf, cwd: String) -> Self {
        let ordinal = std::fs::read_to_string(&path).map(|text| text.lines().count()).unwrap_or(0);
        Self { path, cwd, records: codex_records(), ordinal, fresh: false }
    }

    /// Append the `instructions`, `request`, `reply`, `complete` or `limit`
    /// line, with `text` as the request's or reply's words.
    fn write(&mut self, kind: &str, text: &str) {
        let mut line = self.records[kind].clone();
        match kind {
            "instructions" => {
                let context = line["payload"]["content"][1]["text"].as_str().unwrap_or_default().replace("/work/project", &self.cwd);
                line["payload"]["content"][1]["text"] = context.into();
            }
            "request" | "reply" => line["payload"]["content"][0]["text"] = text.into(),
            _ => {}
        }
        self.put(line);
    }

    fn put(&mut self, mut line: Value) {
        line["timestamp"] = now().into();
        line["ordinal"] = self.ordinal.into();
        self.ordinal += 1;
        append(&self.path, &line);
    }
}

fn codex_records() -> Value {
    serde_json::from_str(CODEX_RECORDS).expect("codex.json")
}

/// The rollout of thread `id` under `dir`, where Codex looks it up.
fn rollout(dir: &Path, id: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = rollout(&path, id) {
                return Some(found);
            }
        } else if path.to_string_lossy().ends_with(&format!("-{id}.jsonl")) {
            return Some(path);
        }
    }
    None
}

/// The words no option claims, in order; everything after `--` is one.
fn positionals(args: &[String], one: &[&str], many: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut words = args.iter().peekable();
    while let Some(word) = words.next() {
        if word == "--" {
            out.extend(words.by_ref().cloned());
        } else if !word.starts_with('-') {
            out.push(word.clone());
        } else if one.contains(&word.as_str()) {
            words.next();
        } else if many.contains(&word.as_str()) {
            while words.next_if(|next| !next.starts_with('-')).is_some() {}
        }
    }
    out
}

/// Claude Code's folder for a working directory's sessions: every character
/// but an ASCII letter or digit becomes `-`.
pub fn encode(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn home_of(variable: &str, default: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(default))
}

/// What `--version` says: `<state>/<program>-version`, or the newest version
/// FubuKing's handoff is tested with.
fn version(program: &str, provider: Provider) -> String {
    std::fs::read_to_string(state().join(format!("{program}-version")))
        .map(|version| version.trim().to_string())
        .unwrap_or_else(|_| fubuking::doctor::tested(provider).1.to_string())
}

fn plan(home: &Path) -> String {
    std::fs::read_to_string(home.join(PLAN)).map(|plan| plan.trim().to_string()).unwrap_or_default()
}

fn cwd() -> String {
    std::env::current_dir().expect("working directory").to_string_lossy().into_owned()
}

/// The clock both CLIs stamp lines with: UTC, to the millisecond.
fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn state() -> PathBuf {
    PathBuf::from(std::env::var_os(STATE).expect(STATE))
}

/// A new id, from a counter the test's runs share: a UUID for Claude, a
/// thread id for Codex.
fn next_id() -> String {
    let counter = state().join("ids");
    let next = std::fs::read_to_string(&counter).ok().and_then(|n| n.trim().parse::<u64>().ok()).unwrap_or(0) + 1;
    std::fs::write(&counter, next.to_string()).expect("id counter");
    format!("00000000-0000-4000-8000-{next:012}")
}

fn record(program: &str, home: &Path, args: &[String]) {
    let call = Call { program: program.to_string(), home: home.to_path_buf(), args: args.to_vec() };
    let mut line = serde_json::to_string(&call).expect("call");
    line.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(state().join("calls.jsonl"))
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .expect("recording the run");
}

fn append(path: &Path, line: &Value) {
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("session folder");
    let mut text = line.to_string();
    text.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .expect("writing the session");
}

/// An interactive CLI stays open after a limit until FubuKing stops it. After
/// a minute it gives up, so a stop that never comes fails the test instead of
/// hanging it.
fn wait_to_be_stopped() -> i32 {
    std::thread::sleep(Duration::from_secs(60));
    3
}
