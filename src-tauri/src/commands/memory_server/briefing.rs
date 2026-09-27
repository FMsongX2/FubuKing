//! What a session pulls from the record: the session-start briefing
//! (`memory_briefing`) and what other sessions recorded since it last looked
//! (`memory_changes`).
//!
//! - **The briefing** is working memory (the active plan, then files changed
//!   newest first) and the **durable index**: each durable kind's best entries
//!   up to its display cap, ranked by [`score`] — recency with a two-week
//!   half-life, use count and confidence — and admitted best first across
//!   kinds within [`INDEX_MAX_ENTRIES`] and [`INDEX_MAX_CHARS`] of content. An
//!   index line carries a capped `content`; `memory_get` has the rest.
//! - **Changes** are the entries other sessions wrote or edited after the
//!   session's last look, newest first, at most [`CHANGES_MAX_PER_KIND`] of
//!   each kind. The session's own writes are left out: it made them.
//! - **The clock** ([`SessionClocks`]) is the newest `updated_at` a session
//!   has seen, kept per session id from its briefing or last changes call and
//!   dropped when the session ends. Storage is unbounded; only what one read
//!   ranks is bounded ([`RANK_POOL`]).
//!
//! Ranking and admission are pure over entries so they are testable without
//! a store; the two `read_*` functions are the only ones that touch SQLite
//! (blocking — callers run them off the async runtime).

use std::collections::{HashMap, HashSet};

use atlas_memory::record::{Entry, EntryKind, Origin, RecordStore};
use parking_lot::Mutex;
use serde_json::{json, Value};

/// The index never carries more entries than this.
pub(super) const INDEX_MAX_ENTRIES: usize = 200;
/// The index's content budget, index lines summed.
pub(super) const INDEX_MAX_CHARS: usize = 8_000;
/// One index line's content cap — an index names what exists, the entry
/// itself is a `memory_get` away.
pub(super) const INDEX_ENTRY_MAX_CHARS: usize = 160;
/// The active plan's content cap in a briefing.
const PLAN_MAX_CHARS: usize = 2_000;
/// How many entries of one kind a changes call carries.
pub(super) const CHANGES_MAX_PER_KIND: usize = 8;
/// Recency half-life for ranking: two weeks.
const HALF_LIFE_MS: f64 = 14.0 * 24.0 * 60.0 * 60.0 * 1000.0;
/// How many entries of one kind are read before ranking or filtering.
const RANK_POOL: usize = 5_000;
/// The durable kinds, in the order the index groups them.
pub(super) const DURABLE_KINDS: [EntryKind; 4] =
    [EntryKind::Decision, EntryKind::Fact, EntryKind::Failure, EntryKind::Architecture];

// ── Clocks ───────────────────────────────────────────────────────────────────

/// Each session's "last looked" clock: the newest `updated_at` it has seen
/// through `memory_briefing` or `memory_changes`. Keyed by session id.
///
/// Deliberately still only the clock (ADR-0010 defines it as exactly that).
/// Whether a session read memory *at all* is a different question with a
/// different answer, and lives in [`SessionReads`].
#[derive(Default)]
pub struct SessionClocks(Mutex<HashMap<String, i64>>);

impl SessionClocks {
    /// When `session_id` last looked; `None` before its first briefing or
    /// changes call.
    pub fn last_look(&self, session_id: &str) -> Option<i64> {
        self.0.lock().get(session_id).copied()
    }

    /// Record that `session_id` has now seen everything up to `at`. Monotonic.
    pub fn looked(&self, session_id: &str, at: i64) {
        let mut clocks = self.0.lock();
        let clock = clocks.entry(session_id.to_string()).or_insert(at);
        if at > *clock {
            *clock = at;
        }
    }

    /// Drop `session_id`'s clock (its session ended).
    pub fn forget(&self, session_id: &str) {
        self.0.lock().remove(session_id);
    }
}

/// Which sessions have read shared memory.
///
/// Separate from [`SessionClocks`] because it answers a different question.
/// The clock moves only on a briefing or a changes call, so a session that
/// answered perfectly well from `memory_search` has no clock at all — and
/// reading "never looked" off the clock would accuse an agent of ignoring
/// memory it had just used. Every read counts here, and writes do not:
/// recording a fact is not looking at what was already there.
#[derive(Default)]
pub struct SessionReads {
    read: Mutex<HashSet<String>>,
}

impl SessionReads {
    /// Record that `session_id` read memory, by whichever tool.
    pub fn read(&self, session_id: &str) {
        self.read.lock().insert(session_id.to_string());
    }

    /// Whether `session_id` has read memory at all. Asserted by the tests
    /// that pin "writing is not reading"; nothing in the app asks any more.
    #[cfg(test)]
    pub fn has_read(&self, session_id: &str) -> bool {
        self.read.lock().contains(session_id)
    }

    /// Drop what is remembered about `session_id` (its session ended).
    #[cfg(test)]
    pub fn forget(&self, session_id: &str) {
        self.read.lock().remove(session_id);
    }
}

// ── Ranking ──────────────────────────────────────────────────────────────────

/// An entry's index rank: recency (two-week half-life, from its last use or
/// write, whichever is later) + ln(1 + use count) + confidence.
pub(super) fn score(e: &Entry, now: i64) -> f64 {
    let last = e.last_used_at.unwrap_or(0).max(e.updated_at);
    let age = now.saturating_sub(last).max(0) as f64;
    0.5f64.powf(age / HALF_LIFE_MS) + (1.0 + f64::from(e.uses)).ln() + e.confidence
}

/// The index over `entries` (any mix of kinds; working memory is ignored):
/// each durable kind's best entries up to its display cap, then, best first
/// across kinds, as many as fit [`INDEX_MAX_ENTRIES`] and
/// [`INDEX_MAX_CHARS`]. Returned grouped by kind in [`DURABLE_KINDS`] order,
/// best first within a kind.
pub(super) fn rank_index(entries: &[Entry], now: i64) -> Vec<Entry> {
    let mut pool: Vec<(f64, &Entry)> = Vec::new();
    for kind in DURABLE_KINDS {
        let mut of_kind: Vec<(f64, &Entry)> =
            entries.iter().filter(|e| e.kind == kind).map(|e| (score(e, now), e)).collect();
        of_kind.sort_by(|a, b| b.0.total_cmp(&a.0));
        of_kind.truncate(kind.cap());
        pool.extend(of_kind);
    }
    pool.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut chars = 0usize;
    let mut admitted: Vec<Vec<&Entry>> = vec![Vec::new(); DURABLE_KINDS.len()];
    let mut count = 0usize;
    for (_, e) in pool {
        let Some(slot) = DURABLE_KINDS.iter().position(|k| *k == e.kind) else { continue };
        let cost = one_line(&e.content, INDEX_ENTRY_MAX_CHARS).chars().count();
        if count + 1 > INDEX_MAX_ENTRIES || chars + cost > INDEX_MAX_CHARS {
            continue;
        }
        count += 1;
        chars += cost;
        admitted[slot].push(e);
    }
    admitted.into_iter().flatten().cloned().collect()
}

// ── Reads ────────────────────────────────────────────────────────────────────

/// What a session-start briefing carries from the record.
#[derive(Debug, Default)]
pub(super) struct Briefing {
    pub plan: Option<Entry>,
    /// Newest first.
    pub files_changed: Vec<Entry>,
    /// Grouped by kind, best first within a kind.
    pub index: Vec<Entry>,
    /// The newest `updated_at` read: what the session has now seen.
    pub synced_to: i64,
}

/// The briefing, from the record. Blocking (SQLite).
pub(super) fn read_briefing(store: &RecordStore, now: i64) -> anyhow::Result<Briefing> {
    let plan = store.list(EntryKind::Plan, 1, Origin::Any)?.pop();
    let mut files_changed = store.list(EntryKind::FileChanged, EntryKind::FileChanged.cap(), Origin::Any)?;
    files_changed.reverse();
    let mut durable = Vec::new();
    for kind in DURABLE_KINDS {
        durable.extend(store.list(kind, RANK_POOL, Origin::Any)?);
    }
    let synced_to = plan
        .iter()
        .chain(&files_changed)
        .chain(&durable)
        .map(|e| e.updated_at)
        .max()
        .unwrap_or(0);
    Ok(Briefing { plan, files_changed, index: rank_index(&durable, now), synced_to })
}

/// What other sessions recorded since a session last looked.
#[derive(Debug, Default)]
pub(super) struct Changes {
    pub since: i64,
    pub synced_to: i64,
    /// Newest first, at most [`CHANGES_MAX_PER_KIND`] of each kind.
    pub entries: Vec<Entry>,
}

/// Entries written or edited after `since` by sessions other than
/// `own_session`. Blocking (SQLite).
pub(super) fn read_changes(store: &RecordStore, since: i64, own_session: &str) -> anyhow::Result<Changes> {
    let mut synced_to = since;
    let mut entries: Vec<Entry> = Vec::new();
    for kind in EntryKind::ALL {
        let limit = if kind == EntryKind::Plan { 1 } else { RANK_POOL };
        let of_kind = store.list(kind, limit, Origin::Any)?;
        synced_to = of_kind.iter().map(|e| e.updated_at).fold(synced_to, i64::max);
        entries.extend(
            of_kind
                .into_iter()
                .rev()
                .filter(|e| e.updated_at > since && e.session_id != own_session)
                .take(CHANGES_MAX_PER_KIND),
        );
    }
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
    Ok(Changes { since, synced_to, entries })
}

// ── Wire shapes ──────────────────────────────────────────────────────────────

/// Who a memory is from: the agent that wrote it, else its source
/// (`import:memdir`, `user`, `extractor`, …).
pub(super) fn provenance(e: &Entry) -> &str {
    if e.agent.trim().is_empty() {
        &e.source
    } else {
        &e.agent
    }
}

/// One entry as every tool returns it.
pub(super) fn entry_json(e: &Entry) -> Value {
    let mut value = json!({
        "id": e.id,
        "kind": e.kind.as_str(),
        "key": e.key,
        "content": e.content,
        "by": provenance(e),
        "source": e.source,
        "confidence": e.confidence,
        "updatedAt": e.updated_at,
        "uses": e.uses,
    });
    if e.kind == EntryKind::Plan && !e.status.is_empty() {
        value["status"] = json!(e.status);
    }
    value
}

/// One entry as the index lists it: `content` capped at `max_chars`, with
/// `truncated: true` when it was, so the agent knows `memory_get` has more.
fn capped_json(e: &Entry, max_chars: usize) -> Value {
    let flat = one_line(&e.content, max_chars);
    let truncated = flat.ends_with('…') && e.content.chars().count() > max_chars;
    let mut value = json!({
        "id": e.id,
        "kind": e.kind.as_str(),
        "content": flat,
        "by": provenance(e),
        "confidence": e.confidence,
        "updatedAt": e.updated_at,
    });
    if truncated {
        value["truncated"] = json!(true);
    }
    value
}

/// The `memory_briefing` result, before the first-look extras are added.
pub(super) fn briefing_json(b: &Briefing) -> Value {
    let plan = b.plan.as_ref().map(|p| {
        let content = truncate_chars(p.content.trim(), PLAN_MAX_CHARS);
        let mut value = json!({
            "id": p.id,
            "content": content,
            "status": p.status,
            "by": provenance(p),
            "updatedAt": p.updated_at,
        });
        if p.content.trim().chars().count() > PLAN_MAX_CHARS {
            value["truncated"] = json!(true);
        }
        value
    });
    let files: Vec<Value> = b
        .files_changed
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "path": c.key,
                "summary": one_line(&c.content, INDEX_ENTRY_MAX_CHARS),
                "by": provenance(c),
                "updatedAt": c.updated_at,
            })
        })
        .collect();
    let mut index = serde_json::Map::new();
    for kind in DURABLE_KINDS {
        let of_kind: Vec<Value> =
            b.index.iter().filter(|e| e.kind == kind).map(|e| capped_json(e, INDEX_ENTRY_MAX_CHARS)).collect();
        if !of_kind.is_empty() {
            index.insert(kind.as_str().to_string(), Value::Array(of_kind));
        }
    }
    json!({
        "plan": plan,
        "filesChanged": files,
        "index": index,
        "syncedTo": b.synced_to,
    })
}

/// The `memory_changes` result.
pub(super) fn changes_json(c: &Changes) -> Value {
    json!({
        "since": c.since,
        "syncedTo": c.synced_to,
        "entries": c.entries.iter().map(entry_json).collect::<Vec<_>>(),
    })
}

/// Whitespace collapsed to single spaces, capped at `max` chars with `…`.
fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&flat, max)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
