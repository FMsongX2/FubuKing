// Modified by FubuKing from upstream Atlas (Apache-2.0).
//! The Shared-tab command contract, pinned byte-for-byte.
//!
//! The five commands (`memory_get_state`, `memory_list_events`, `memory_query`,
//! `memory_append_event`, `memory_clear_project`) are thin wrappers over the
//! store methods driven here, and Tauri serialises their return values with
//! serde_json — so the JSON of these calls IS the response the Memory panel
//! receives. The goldens under `testdata/` were captured from the
//! JSONL event-log store before the record store replaced it; the record store
//! has to reproduce them exactly.
//!
//! Re-bless (only when the contract is meant to change) with
//! `ATLAS_BLESS_SHARED_MEMORY_GOLDENS=1`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

use super::{EventKind, RawEvent, SharedMemoryStore};

/// A store whose clock ticks one second per event from a fixed epoch, so
/// timestamps in the responses are reproducible.
fn ticking_store() -> SharedMemoryStore {
    let t = Arc::new(AtomicI64::new(1_750_000_000_000));
    SharedMemoryStore::with_clock(Arc::new(move || t.fetch_add(1000, Ordering::SeqCst)))
}

fn temp_project(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atlas-shared-contract-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

/// Objects with their keys sorted. `sessionAgents` is a `HashMap`, whose
/// iteration order was never stable, so comparison is key-order-insensitive
/// there and everywhere else is compared as serialised.
fn canonical(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::new();
            for (k, v) in entries {
                out.insert(k, canonical(v));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
        other => other,
    }
}

fn append(
    store: &SharedMemoryStore,
    project: &str,
    agent: &str,
    session: &str,
    kind: EventKind,
    key: &str,
    payload: Value,
) -> u64 {
    store
        .append_event(
            project,
            RawEvent {
                agent: agent.into(),
                session_id: session.into(),
                kind,
                key: key.into(),
                payload,
            },
        )
        .unwrap()
}

/// Snapshot every read command's response for `project`.
fn snapshot(store: &SharedMemoryStore, project: &str) -> Value {
    json!({
        "memory_get_state": serde_json::to_value(store.get_state(project)).unwrap(),
        "memory_list_events": serde_json::to_value(store.list_events(project)).unwrap(),
        "memory_query:auth": serde_json::to_value(store.query(project, "auth", 20)).unwrap(),
        "memory_query:CODEX/3": serde_json::to_value(store.query(project, "CODEX", 3)).unwrap(),
        "memory_query:empty/0": serde_json::to_value(store.query(project, "  ", 0)).unwrap(),
        "memory_query:nothing": serde_json::to_value(store.query(project, "zzz-no-hit", 20)).unwrap(),
    })
}

fn check_golden(name: &str, actual: Value) {
    let actual = serde_json::to_string_pretty(&canonical(actual)).unwrap();
    let path = testdata().join(name);
    if std::env::var_os("ATLAS_BLESS_SHARED_MEMORY_GOLDENS").is_some() {
        std::fs::write(&path, format!("{actual}\n")).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(actual, expected.trim_end(), "golden {name} differs");
}

/// The scripted append sequence: replace-by-key, keyless text dedup, repeat
/// edits to one path, plan set/replace/clear, session bookkeeping, events that
/// fold to nothing, and enough decisions and failures to pass the caps.
fn scripted_sequence(store: &SharedMemoryStore, p: &str) -> Vec<u64> {
    let mut seqs = Vec::new();
    let mut a = |agent: &str, session: &str, kind: EventKind, key: &str, payload: Value| {
        seqs.push(append(store, p, agent, session, kind, key, payload));
    };
    a("claude-code", "s1", EventKind::SessionStart, "", json!({}));
    a("codex", "s2", EventKind::SessionStart, "", json!({}));
    a("claude-code", "s1", EventKind::PlanSet, "plan", json!({"text": "- [pending] Plan A", "status": "active"}));
    a("codex", "s2", EventKind::PlanSet, "plan", json!({"text": "- [pending] Plan B", "status": "in_progress"}));
    a("claude-code", "s1", EventKind::PlanSet, "plan", json!({"text": "", "status": "active"}));
    a("claude-code", "s1", EventKind::Decision, "auth.alg", json!({"text": "Sign JWTs with HS256"}));
    a("codex", "s2", EventKind::Decision, "auth.alg", json!({"text": "Sign JWTs with RS256"}));
    a("codex", "s2", EventKind::Decision, "", json!({"text": "Use   Postgres 16"}));
    a("claude-code", "s1", EventKind::Decision, "", json!({"text": "use postgres 16"}));
    a("claude-code", "s1", EventKind::Decision, "", json!({"text": "   "}));
    a("codex", "s2", EventKind::FileChanged, "src/a.ts", json!({"path": "src/a.ts", "summary": "Edit src/a.ts"}));
    a("claude-code", "s1", EventKind::FileChanged, "src/a.ts", json!({"path": "src/a.ts", "summary": "Rewrite src/a.ts"}));
    a("claude-code", "s1", EventKind::FileChanged, "src/b.ts", json!({"summary": "Create src/b.ts"}));
    a("claude-code", "s1", EventKind::FileChanged, "", json!({"summary": "no path"}));
    a("codex", "s2", EventKind::Fact, "port", json!({"text": "The API listens on 4747"}));
    a("claude-code", "s1", EventKind::Fact, "other", json!({"text": "the api listens on 4747"}));
    a("codex", "s2", EventKind::Failure, "", json!({"text": "npm ci fails offline"}));
    a("claude-code", "s1", EventKind::Failure, "npm ci fails offline", json!({"text": "Offline installs need the cache"}));
    a("codex", "s2", EventKind::Architecture, "", json!({"text": "The backend is the only writer"}));
    a("codex", "s2", EventKind::Architecture, "", json!({"text": "The  backend is the ONLY writer"}));
    a("codex", "s2", EventKind::TodoAdded, "", json!({"text": "write docs"}));
    a("codex", "s2", EventKind::TodoDone, "", json!({"text": "write docs"}));
    a("codex", "s2", EventKind::SessionEnd, "", json!({}));
    a("codex", "s2", EventKind::PlanSet, "plan", json!({"text": "- [completed] Plan B", "status": "done"}));
    for i in 1..=55 {
        a("codex", "s2", EventKind::Decision, &format!("k{i}"), json!({"text": format!("decision number {i}")}));
    }
    a("claude-code", "s1", EventKind::Decision, "k30", json!({"text": "decision thirty, revised"}));
    for i in 1..=34 {
        a("claude-code", "s1", EventKind::Failure, "", json!({"text": format!("failure number {i}")}));
    }
    for i in 1..=52 {
        a("codex", "s2", EventKind::FileChanged, &format!("src/f{i}.rs"), json!({"path": format!("src/f{i}.rs"), "summary": format!("Edit f{i}")}));
    }
    for i in 1..=31 {
        a("codex", "s2", EventKind::Architecture, "", json!({"text": format!("module {i} owns its state")}));
    }
    for i in 1..=51 {
        a("claude-code", "s1", EventKind::Fact, "", json!({"text": format!("fact number {i}")}));
    }
    a("claude-code", "s1", EventKind::PlanSet, "plan", json!({"text": "- [pending] Plan C", "status": "active"}));
    seqs
}

#[test]
fn scripted_appends_reproduce_the_shared_tab_contract() {
    let dir = temp_project("script");
    let p = dir.to_string_lossy().to_string();
    let store = ticking_store();

    let seqs = scripted_sequence(&store, &p);
    let mut golden = json!({ "memory_append_event": seqs });
    golden["after_appends"] = snapshot(&store, &p);
    golden["last_seq"] = json!(store.get_state(&p).last_seq);

    // Clear wipes the record; the next append starts the sequence again.
    store.clear(&p).unwrap();
    golden["after_clear"] = snapshot(&store, &p);
    golden["append_after_clear"] = json!(append(&store, &p, "codex", "s3", EventKind::Fact, "", json!({"text": "fresh start"})));
    golden["after_clear_and_append"] = snapshot(&store, &p);

    check_golden("contract.golden.json", golden);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A project whose shared memory was written by the JSONL store (event log +
/// extracted memdir) answers the five commands exactly as it did before, and
/// keeps doing so across reopen; appends continue the old sequence.
#[test]
fn a_legacy_project_answers_the_contract_unchanged() {
    let dir = temp_project("legacy");
    seed_legacy_project(&dir);
    let p = dir.to_string_lossy().to_string();

    let first = snapshot(&ticking_store(), &p);
    check_golden("legacy.golden.json", first.clone());

    // A second store (a relaunch) sees the same record, not a doubled one.
    let reopened = ticking_store();
    assert_eq!(canonical(snapshot(&reopened, &p)), canonical(first));
    let seq = append(&reopened, &p, "codex", "s9", EventKind::Decision, "", json!({"text": "after migration"}));
    assert_eq!(seq, 12);
    let _ = std::fs::remove_dir_all(&dir);
}

pub(super) fn seed_legacy_project(dir: &Path) {
    let shared = dir.join(".atlas/shared-memory");
    let extracted = dir.join(".atlas/memory/extracted");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::create_dir_all(&extracted).unwrap();
    let src = testdata().join("legacy");
    std::fs::copy(src.join("events.jsonl"), shared.join("events.jsonl")).unwrap();
    // Stored as `.md.fixture`: the repo ignores `*.md` outside docs.
    for f in ["sess-a.md", "sess-b.md"] {
        std::fs::copy(src.join("extracted").join(format!("{f}.fixture")), extracted.join(f)).unwrap();
    }
}
