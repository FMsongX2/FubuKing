//! The read model: turning the capture tables back into something a developer
//! can look at.
//!
//! Everything else in this crate writes. Nothing read it back. A recorder with
//! no viewer is indistinguishable from a recorder that does not work, which is
//! the whole reason this module exists — the store already held every fact these
//! shapes carry, and none of it had a way out.
//!
//! Two shapes, matching the two things a developer wants:
//!
//! * [`SessionSummary`] — one row per Session, cheap enough to list hundreds.
//!   Counts come from indexed aggregates; no message body or blob is touched.
//! * [`SessionDetail`] — one Session in full, as a single ordered timeline of
//!   prompts, responses, tool calls and Checkpoints.
//!
//! Both are deliberately **flat and serialisable**: they cross the Tauri
//! boundary as JSON, so they hold resolved strings rather than ids the frontend
//! would have to join.
//!
//! ## Why bodies are inlined conditionally
//!
//! A message body can be a 40 MB pasted log. Sending every body for every
//! message would make opening a Session slower than the work it recorded. So a
//! body is inlined when it is small, and otherwise the 2 KB preview is sent with
//! [`TimelineEntry::truncated`] set — the viewer shows what it has and says the
//! rest is on disk. This is the same trade the store makes when it spills.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::{Checkpoint, LinkState, Message, Mode, Role, Session, ToolCall, ToolStatus};
use crate::store::Store;

/// Largest body inlined into a timeline entry.
///
/// Above this the preview is sent instead. 64 KB is generous for a prompt or a
/// response and small enough that a hundred of them still fit in one payload.
pub const INLINE_LIMIT_BYTES: i64 = 64 * 1024;

/// One row in the Sessions list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    /// Derived from the first prompt at capture time. `None` until one arrives.
    pub title: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    /// `acp`, `native` or `external_jsonl` — where the record came from.
    pub source: String,
    pub started_at: String,
    pub updated_at: String,
    /// When the Session last did work. The board orders and groups on this —
    /// never on `updated_at`, which moves whenever the row is rewritten.
    pub last_activity_at: String,
    /// Agent time: the sum of the Session's turns when the agent reported
    /// them, otherwise the gap-capped sum of its message intervals.
    ///
    /// This used to be `updated_at - started_at`, which is neither agent time
    /// nor an honest span — a transcript imported six weeks after it ran
    /// reported the six weeks. See [`IDLE_CAP_SECONDS`] and
    /// [`TURN_CAP_SECONDS`] for what the caps are protecting against.
    pub active_seconds: i64,
    /// First activity to last, idle included. Usually much larger than
    /// `active_seconds`, and still worth showing — it is the answer to "when
    /// was I working on this", which agent time cannot give.
    pub wall_seconds: i64,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub checkpoint_count: i64,
    /// Every branch this Session touched: the one it started on, plus every
    /// branch its Checkpoints landed on, deduplicated. The first entry is the
    /// starting branch when there is one, so a row can show a single branch
    /// without picking arbitrarily.
    pub branches: Vec<String>,
    pub insertions: i64,
    pub deletions: i64,
    pub files_touched: i64,
    /// Input + output. Zero for an agent that reports no split — see
    /// `context_used`.
    pub total_tokens: i64,
    /// The two halves of `total_tokens`, carried separately because they are
    /// priced separately: for Opus 5 an output token costs five times an input
    /// one, so a viewer handed only the sum cannot estimate a cost at all.
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Cache writes and cache reads, carried beside the split rather than
    /// inside it. They are real spend and were being dropped on the floor, but
    /// folding them into `total_tokens` would make "in + out" mean something
    /// else — for a cache-heavy agent they dwarf both.
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    /// Context-window occupancy, for agents that report only that.
    ///
    /// ACP agents (Claude Code, Codex) do not emit a usage split, so
    /// `total_tokens` is 0 for them and this is the only figure there is. It is
    /// kept separate rather than folded into the total because occupancy is not
    /// consumption — a compaction drops it, and presenting it as tokens spent
    /// would be a lie that gets worse the longer a Session runs.
    pub context_used: Option<i64>,
    pub context_size: Option<i64>,
    /// Something could not be recorded. The viewer surfaces this per row so a
    /// hole in the record is visible where the record is read.
    pub needs_attention: bool,
    pub attention_reason: Option<String>,
}

/// What kind of thing a timeline entry is. Drives both the rendering and the
/// filter checkboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A message the developer sent.
    Prompt,
    /// A message the agent sent back.
    Response,
    /// Agent reasoning — collapsed by default, because it is the bulk of the
    /// bytes and rarely what someone opened the Session to read.
    Thinking,
    /// A tool the agent invoked.
    ToolCall,
    /// A commit linked to this Session.
    Checkpoint,
}

/// One thing that happened, in order.
///
/// A single flat shape rather than an enum-per-kind: the viewer renders these in
/// one list, and a tagged union would push the same field-presence checks into
/// the frontend without removing them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub id: String,
    pub kind: EntryKind,
    pub at: String,
    /// Turn this belongs to. Checkpoints report the turn of the tool call that
    /// touched their files, or `-1` when they could not be attributed to one.
    pub turn_seq: i64,

    // ── Messages ────────────────────────────────────────────────────────────
    /// Full body when it fit, otherwise the preview. Always redacted — scrubbing
    /// happened before persistence, so there is nothing left to scrub here.
    pub text: Option<String>,
    /// The body was too large to inline and `text` is the preview.
    pub truncated: bool,
    pub body_bytes: i64,
    /// Blob key of the full body when it was spilled — the handle the viewer
    /// uses to fetch the rest on demand.
    pub body_ref: Option<String>,

    // ── Tool calls ──────────────────────────────────────────────────────────
    /// Canonical name — `read`, `edit`, `bash`, … — not the wire name, which
    /// differs per agent.
    pub tool_name: Option<String>,
    /// The agent's own display title, when it sent one.
    pub tool_title: Option<String>,
    pub tool_status: Option<ToolStatus>,
    /// Files the call touched, for the one-line summary next to the tool name.
    pub paths: Vec<String>,
    pub arguments: Option<String>,
    pub arguments_ref: Option<String>,
    pub result: Option<String>,
    /// Blob key of the full result when it was spilled.
    pub result_ref: Option<String>,
    /// The result was binary and is not shown.
    pub result_binary: bool,

    // ── Checkpoints ─────────────────────────────────────────────────────────
    pub commit_sha: Option<String>,
    /// Read from git at display time. Commit messages are git's to own; copying
    /// them into the store would just create a second version that goes stale
    /// after a reword.
    pub commit_subject: Option<String>,
    pub branch: Option<String>,
    pub link_state: Option<LinkState>,
    pub insertions: i64,
    pub deletions: i64,
    pub files: Vec<String>,
}

impl TimelineEntry {
    /// An entry with every optional field empty, to be filled by the builders.
    fn blank(id: String, kind: EntryKind, at: String, turn_seq: i64) -> Self {
        Self {
            id,
            kind,
            at,
            turn_seq,
            text: None,
            truncated: false,
            body_bytes: 0,
            body_ref: None,
            tool_name: None,
            tool_title: None,
            tool_status: None,
            paths: Vec::new(),
            arguments: None,
            arguments_ref: None,
            result: None,
            result_ref: None,
            result_binary: false,
            commit_sha: None,
            commit_subject: None,
            branch: None,
            link_state: None,
            insertions: 0,
            deletions: 0,
            files: Vec::new(),
        }
    }
}

/// How many entries of each kind a Session holds.
///
/// Sent alongside the entries so the filter checkboxes can show counts without
/// the frontend recomputing them, and so a filtered-to-empty timeline can still
/// say what it is hiding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryCounts {
    pub prompts: i64,
    pub responses: i64,
    pub thinking: i64,
    pub tool_calls: i64,
    pub checkpoints: i64,
}

/// One Session, in full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub entries: Vec<TimelineEntry>,
    pub counts: EntryCounts,
    /// Per-canonical-tool totals, for the nested filter rows under "Tool calls".
    pub tools: Vec<ToolTally>,
}

/// How many times one tool was called.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTally {
    pub tool_name: String,
    pub count: i64,
}

/// Every Session in a Project, newest first.
///
/// Ordered by `updated_at` rather than `started_at`: a Session resumed today is
/// today's work, whatever day it began on.
/// Every Session in a Project, newest first.
///
/// Four queries regardless of how many Sessions there are. It used to be
/// `3n + 1` — three per row — which was invisible for one Project and became
/// the whole cost once the board started spanning every project in an
/// Organisation. The totals and the Checkpoints are fetched in one pass each
/// and matched up in memory.
pub fn sessions(store: &Store, workspace_id: &str) -> Result<Vec<SessionSummary>> {
    let message_counts = store.message_counts(workspace_id)?;
    let tool_call_counts = store.tool_call_counts_by_session(workspace_id)?;
    let turn_time = store.turn_active_seconds(workspace_id, TURN_CAP_SECONDS)?;
    let message_time = store.message_active_seconds(workspace_id, IDLE_CAP_SECONDS)?;

    let mut by_session: HashMap<String, Vec<Checkpoint>> = HashMap::new();
    for checkpoint in store.checkpoints_for_project(workspace_id)? {
        by_session.entry(checkpoint.session_id.clone()).or_default().push(checkpoint);
    }

    let mut out = Vec::new();
    for session in store.sessions_for_project(workspace_id)? {
        let checkpoints = by_session.get(&session.id).map(Vec::as_slice).unwrap_or(&[]);
        out.push(summarize(
            &session,
            checkpoints,
            message_counts.get(&session.id).copied().unwrap_or(0),
            tool_call_counts.get(&session.id).copied().unwrap_or(0),
            active_seconds(
                turn_time.get(&session.id).copied(),
                message_time.get(&session.id).copied().unwrap_or(0),
            ),
        ));
    }
    out.sort_by(|a, b| b.last_activity_at.cmp(&a.last_activity_at));
    Ok(out)
}

/// One Session's summary row, by store id.
///
/// The composer's Usage popup asks for exactly one row while a session is
/// live; the board's one-`GROUP BY`-per-table shape would read the whole
/// Project to answer it. Five point queries over covering indexes instead.
pub fn session_summary(store: &Store, session_id: &str) -> Result<Option<SessionSummary>> {
    let Some(session) = store.session(session_id)? else {
        return Ok(None);
    };
    let checkpoints = store.checkpoints_for_session(session_id)?;
    let message_count = store.message_count(session_id)?;
    let tool_call_count = store.tool_call_count(session_id)?;
    let turns = store.turn_active_seconds_for(session_id, TURN_CAP_SECONDS)?;
    let message_seconds = store.message_active_seconds_for(session_id, IDLE_CAP_SECONDS)?;
    Ok(Some(summarize(
        &session,
        &checkpoints,
        message_count,
        tool_call_count,
        active_seconds(Some(turns), message_seconds),
    )))
}

/// A gap longer than this between two messages is a developer who walked away.
pub const IDLE_CAP_SECONDS: i64 = 300;
/// A turn that "ran" longer than this did not run: its completion event arrived
/// after a sleep, a restart or a crash reconciliation.
pub const TURN_CAP_SECONDS: i64 = 1_800;

/// Which clock a Session's agent time comes from.
///
/// Turn spans when the agent reported turns at all — they are agent time by
/// construction. Imported transcripts have no turn rows, so they fall back to
/// the gap-capped message intervals. The turn *count* is what distinguishes the
/// two cases: a Session with turns that summed to zero seconds really did work
/// for under a second, and must not silently switch clocks.
fn active_seconds(turns: Option<(i64, i64)>, message_seconds: i64) -> i64 {
    match turns {
        Some((seconds, spans)) if spans > 0 => seconds,
        _ => message_seconds,
    }
}

/// One Session's summary row.
///
/// Message and tool-call totals come from `COUNT(*)` over their covering
/// indexes — the list view must never materialise a body just to `.len()` it.
/// Checkpoint rows *are* fetched: a Session has a handful at most, and the
/// branch list, line counts and file set live on them.
fn summarize(
    session: &Session,
    checkpoints: &[Checkpoint],
    message_count: i64,
    tool_call_count: i64,
    active_seconds: i64,
) -> SessionSummary {
    // Starting branch first, then the Checkpoint branches in the order they
    // were first committed to — so a row that shows one branch shows the one
    // the work began on. `checkpoints` arrive oldest first; sorting them
    // alphabetically instead put `feature/dark-mode` ahead of `master` for a
    // Session that began before `git init` and so has no starting branch.
    let mut branches: Vec<String> = session.branch.iter().cloned().collect();
    for branch in checkpoints.iter().filter_map(|c| c.branch.as_ref()) {
        if !branches.contains(branch) {
            branches.push(branch.clone());
        }
    }

    // Distinct paths, not touch events: editing one file four times is one file.
    let mut files: Vec<&str> =
        checkpoints.iter().flat_map(|c| c.files_touched.iter().map(String::as_str)).collect();
    files.sort_unstable();
    files.dedup();

    let totals = &session.token_totals;
    // A Session recorded before the activity column existed, and with neither a
    // message nor a closed turn to backfill from, has only its mutation clock.
    let last_activity = session.last_activity_at.unwrap_or(session.updated_at);
    SessionSummary {
        id: session.id.clone(),
        title: session.title.clone(),
        agent: session.agent.clone(),
        model: session.model.clone(),
        source: session.source.as_str().to_string(),
        started_at: session.started_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
        last_activity_at: last_activity.to_rfc3339(),
        active_seconds,
        wall_seconds: (last_activity - session.started_at).num_seconds().max(0),
        message_count,
        tool_call_count,
        checkpoint_count: checkpoints.len() as i64,
        branches,
        insertions: checkpoints.iter().map(|c| c.insertions).sum(),
        deletions: checkpoints.iter().map(|c| c.deletions).sum(),
        files_touched: files.len() as i64,
        total_tokens: (totals.input_tokens + totals.output_tokens) as i64,
        input_tokens: totals.input_tokens as i64,
        output_tokens: totals.output_tokens as i64,
        cache_creation_tokens: totals.cache_creation_tokens as i64,
        cache_read_tokens: totals.cache_read_tokens as i64,
        context_used: totals.context_used.map(|n| n as i64),
        context_size: totals.context_size.map(|n| n as i64),
        needs_attention: session.needs_attention,
        attention_reason: session.attention_reason.clone(),
    }
}

/// One Checkpoint, flat enough to list.
///
/// A deliberately smaller shape than [`TimelineEntry`]: this is a jump target,
/// not a reading surface. It carries what identifies a commit (sha, subject,
/// branch), what it cost (`insertions`/`deletions`/`files`) and enough to open
/// the Session it belongs to — and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointRow {
    pub session_id: String,
    /// The Session's title, so a row says which work produced the commit.
    pub session_title: Option<String>,
    pub commit_sha: String,
    /// Read from git at display time, like every other commit subject here —
    /// `None` when the repository has moved or the commit is gone.
    pub commit_subject: Option<String>,
    pub branch: Option<String>,
    pub link_state: LinkState,
    pub insertions: i64,
    pub deletions: i64,
    pub files: usize,
    pub at: String,
}

/// The newest Checkpoints across a Project, most recent first.
///
/// `subject_for` is a callback for the same reason it is on [`detail`]: the read
/// model has to work for a Project whose repository has moved, where the rows
/// still render without subjects.
pub fn recent_checkpoints(
    store: &Store,
    workspace_id: &str,
    limit: i64,
    subject_for: impl Fn(&str) -> Option<String>,
) -> Result<Vec<CheckpointRow>> {
    Ok(store
        .recent_checkpoints(workspace_id, limit)?
        .into_iter()
        .map(|(checkpoint, session_title)| CheckpointRow {
            session_id: checkpoint.session_id,
            session_title,
            commit_subject: subject_for(&checkpoint.commit_sha),
            commit_sha: checkpoint.commit_sha,
            branch: checkpoint.branch,
            link_state: checkpoint.link_state,
            insertions: checkpoint.insertions,
            deletions: checkpoint.deletions,
            files: checkpoint.files_touched.len(),
            at: checkpoint.created_at.to_rfc3339(),
        })
        .collect())
}

/// One Session as an ordered timeline.
///
/// `subject_for` resolves a commit sha to its subject line. It is a callback
/// rather than a git call inside this crate because the read model must work for
/// a Project whose repository has moved or been deleted — in which case the
/// Checkpoint still renders, without a subject.
pub fn detail(
    store: &Store,
    session_id: &str,
    subject_for: impl Fn(&str) -> Option<String>,
) -> Result<Option<SessionDetail>> {
    let Some(session) = store.session(session_id)? else {
        return Ok(None);
    };

    let mut entries = Vec::new();
    let mut counts = EntryCounts::default();

    for message in store.messages_for_session(session_id)? {
        let Some(entry) = message_entry(store, &message)? else { continue };
        match entry.kind {
            EntryKind::Prompt => counts.prompts += 1,
            EntryKind::Response => counts.responses += 1,
            EntryKind::Thinking => counts.thinking += 1,
            _ => {}
        }
        entries.push(entry);
    }

    let touches = store.file_touches_for_session(session_id)?;
    for call in store.tool_calls_for_session(session_id)? {
        counts.tool_calls += 1;
        entries.push(tool_call_entry(store, &call, &touches)?);
    }

    // A Checkpoint carries no turn of its own. Attributing it to the turn whose
    // work it holds is what puts a commit *after* that work rather than at the
    // bottom of the Session. The touches the commit consumed name that turn
    // exactly; any touch of the same path would not — a later turn editing the
    // file again would drag every earlier commit of it down to that turn.
    let consuming = store.consuming_turns(session_id)?;
    let checkpoints = store.checkpoints_for_session(session_id)?;
    for checkpoint in &checkpoints {
        counts.checkpoints += 1;
        let turn = consuming.get(&checkpoint.commit_sha).copied().unwrap_or_else(|| {
            // Nothing consumed (a permissive link, or rows from before
            // consumption was tracked): the last turn that touched one of its
            // files before the commit was seen.
            touches
                .iter()
                .filter(|t| t.created_at <= checkpoint.created_at)
                .filter(|t| checkpoint.files_touched.contains(&t.path))
                .map(|t| t.turn_seq)
                .max()
                .unwrap_or(-1)
        });
        entries.push(checkpoint_entry(checkpoint, turn, &subject_for));
    }

    entries.sort_by(order);

    let tools = store
        .tool_call_counts(session_id)?
        .into_iter()
        .map(|(name, count)| ToolTally { tool_name: name.as_str().to_string(), count })
        .collect();

    // One Session, so the per-Session counts are two queries rather than the
    // list view's `3n`.
    let summary = summarize(
        &session,
        &checkpoints,
        store.message_count(session_id)?,
        store.tool_call_count(session_id)?,
        active_seconds(
            Some(store.turn_active_seconds_for(session_id, TURN_CAP_SECONDS)?),
            store.message_active_seconds_for(session_id, IDLE_CAP_SECONDS)?,
        ),
    );
    Ok(Some(SessionDetail { summary, entries, counts, tools }))
}

/// Timeline order: by turn, then by when it happened.
///
/// Turn is the primary key rather than the timestamp because a Checkpoint is
/// created when the commit is *observed*, which can be minutes after the turn
/// that wrote the files — sorting on time alone would float commits away from
/// their work. Unattributed Checkpoints carry turn `-1` and sort to the top,
/// which is correct: they belong to no turn, and burying them would hide the one
/// case a developer needs to notice.
fn order(a: &TimelineEntry, b: &TimelineEntry) -> std::cmp::Ordering {
    a.turn_seq
        .cmp(&b.turn_seq)
        // Within a turn, a Checkpoint always closes it.
        .then(rank(a.kind).cmp(&rank(b.kind)))
        .then(a.at.cmp(&b.at))
        .then(a.id.cmp(&b.id))
}

fn rank(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::Prompt => 0,
        EntryKind::Thinking => 1,
        EntryKind::ToolCall => 2,
        EntryKind::Response => 3,
        EntryKind::Checkpoint => 4,
    }
}

/// What kind of entry a message is, or `None` for one that has no place in
/// the timeline. The one rule for both the timeline and the comment anchors.
pub(crate) fn entry_kind_for(role: Role, mode: Mode) -> Option<EntryKind> {
    match (role, mode) {
        // A tool-mode message duplicates the tool_call row it was derived from.
        (_, Mode::Tool) => None,
        (_, Mode::Thinking) => Some(EntryKind::Thinking),
        (Role::User, _) => Some(EntryKind::Prompt),
        (Role::Assistant, _) => Some(EntryKind::Response),
        // System messages are plumbing the developer did not write and the agent
        // did not say.
        (Role::System, _) => None,
    }
}

/// One commentable row of a Session, with the id the agent knew it by.
///
/// The live chat holds the agent's ids (a message id from the wire, a tool
/// call id) while a comment is anchored on the captured row id. This is the
/// join: cheap enough to re-read after every turn, and carrying nothing a
/// viewer would render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorEntry {
    pub row_id: String,
    pub kind: EntryKind,
    pub turn_seq: i64,
    /// The agent's own id — `native_message_id` or `native_call_id`. A prompt
    /// carries a synthesised one (`prompt-{turn}-{hash}`) that no live message
    /// has, so a prompt is matched by its turn instead.
    pub native_id: Option<String>,
    /// Tool calls only.
    pub tool_name: Option<String>,
}

/// Every commentable row of a Session in **write order** — `(turn_seq, seq)`,
/// not the timeline's rank order. The chat orders its own rows; what it needs
/// from here is which captured rows exist in which turn.
///
/// Rows of a **rewound** turn are left out: a retry took the turn back, so
/// the live chat has no message for it, and counting it would shift every
/// earlier exchange onto the wrong turn when the chat pairs by position.
pub fn anchors(store: &Store, session_id: &str) -> Result<Vec<AnchorEntry>> {
    let rewound = store.rewound_turns(session_id)?;
    let mut out: Vec<(i64, i64, AnchorEntry)> = Vec::new();
    for m in store.message_anchor_rows(session_id)? {
        if rewound.contains(&m.turn_seq) {
            continue;
        }
        let Some(kind) = entry_kind_for(m.role, m.mode) else { continue };
        out.push((
            m.turn_seq,
            m.seq,
            AnchorEntry {
                row_id: m.id,
                kind,
                turn_seq: m.turn_seq,
                native_id: m.native_message_id,
                tool_name: None,
            },
        ));
    }
    for c in store.tool_call_anchor_rows(session_id)? {
        if rewound.contains(&c.turn_seq) {
            continue;
        }
        out.push((
            c.turn_seq,
            c.seq,
            AnchorEntry {
                row_id: c.id,
                kind: EntryKind::ToolCall,
                turn_seq: c.turn_seq,
                native_id: c.native_call_id,
                tool_name: Some(c.tool_name.as_str().to_string()),
            },
        ));
    }
    out.sort_by_key(|(turn_seq, seq, _)| (*turn_seq, *seq));
    Ok(out.into_iter().map(|(_, _, e)| e).collect())
}

/// A message as a timeline entry, or `None` for one that has no place in it.
fn message_entry(store: &Store, message: &Message) -> Result<Option<TimelineEntry>> {
    let Some(kind) = entry_kind_for(message.role, message.mode) else {
        return Ok(None);
    };

    let inline = message.body_bytes <= INLINE_LIMIT_BYTES;
    let text = if inline {
        // Falls back to the preview when the blob is gone: a Session whose blob
        // store was pruned should still render, with less.
        store.message_body(message).ok().filter(|body| !body.is_empty())
    } else {
        None
    };
    let truncated = text.is_none() && !message.preview.is_empty();

    let mut entry = TimelineEntry::blank(
        message.id.clone(),
        kind,
        message.created_at.to_rfc3339(),
        message.turn_seq,
    );
    entry.text = text.or_else(|| Some(message.preview.clone()));
    entry.truncated = truncated;
    entry.body_bytes = message.body_bytes;
    entry.body_ref = message.body_ref.clone();
    Ok(Some(entry))
}

fn tool_call_entry(
    store: &Store,
    call: &ToolCall,
    touches: &[crate::model::FileTouch],
) -> Result<TimelineEntry> {
    let mut entry = TimelineEntry::blank(
        call.id.clone(),
        EntryKind::ToolCall,
        call.created_at.to_rfc3339(),
        call.turn_seq,
    );
    entry.tool_name = Some(call.tool_name.as_str().to_string());
    entry.tool_title = call.title.clone();
    entry.tool_status = Some(call.status);
    entry.paths =
        touches.iter().filter(|t| t.tool_call_id == call.id).map(|t| t.path.clone()).collect();
    entry.arguments = call.arguments.clone();
    entry.arguments_ref = call.arguments_ref.clone();
    entry.result_ref = call.result_ref.clone();
    entry.result_binary = call.result_binary;
    if !call.result_binary {
        entry.result = match store.tool_call_result(call) {
            Ok(Some(bytes)) if bytes.len() as i64 <= INLINE_LIMIT_BYTES => {
                String::from_utf8(bytes).ok()
            }
            _ => call.result.clone(),
        };
    }
    Ok(entry)
}

fn checkpoint_entry(
    checkpoint: &Checkpoint,
    turn_seq: i64,
    subject_for: &impl Fn(&str) -> Option<String>,
) -> TimelineEntry {
    let mut entry = TimelineEntry::blank(
        checkpoint.id.clone(),
        EntryKind::Checkpoint,
        checkpoint.created_at.to_rfc3339(),
        turn_seq,
    );
    entry.commit_sha = Some(checkpoint.commit_sha.clone());
    entry.commit_subject = subject_for(&checkpoint.commit_sha);
    entry.branch = checkpoint.branch.clone();
    entry.link_state = Some(checkpoint.link_state);
    entry.insertions = checkpoint.insertions;
    entry.deletions = checkpoint.deletions;
    entry.files = checkpoint.files_touched.clone();
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_expose_native_ids_and_skip_tool_and_system_messages() {
        use crate::capture::{Capture, SessionKey, ToolCallContent, TurnContent};
        use crate::model::{ProjectMode, Source, ToolStatus};
        use crate::tools::ToolName;

        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join(".atlas")).unwrap();
        let mut capture = Capture::new(&mut store, ProjectMode::Local);
        let key = SessionKey {
            workspace_id: "ws".into(),
            source: Source::Acp,
            native_session_id: "native-1".into(),
        };
        let session_id = capture.ensure_session(&key, None, None, None, None).unwrap();
        capture.record_prompt(&key, "index the project", 1, None, None, None).unwrap();
        let turn = |native: &str, role: Role, mode: Mode, body: &str| TurnContent {
            turn_seq: 1,
            native_message_id: Some(native.into()),
            role,
            mode,
            body: body.into(),
            created_at: None,
        };
        capture.record_turn(&session_id, turn("th-1", Role::Assistant, Mode::Thinking, "hmm")).unwrap();
        capture.record_turn(&session_id, turn("tool-1", Role::Assistant, Mode::Tool, "ran")).unwrap();
        capture.record_turn(&session_id, turn("sys-1", Role::System, Mode::Text, "plumbing")).unwrap();
        capture.record_turn(&session_id, turn("m-1", Role::Assistant, Mode::Text, "done")).unwrap();
        let locations = serde_json::Value::Null;
        capture
            .record_tool_call(
                &session_id,
                ToolCallContent {
                    turn_seq: 1,
                    native_call_id: Some("call-1"),
                    tool_name: ToolName::Read,
                    title: None,
                    kind: None,
                    status: ToolStatus::Completed,
                    locations: &locations,
                    arguments: None,
                    result: None,
                },
            )
            .unwrap();

        let got = anchors(&store, &session_id).unwrap();
        let kinds: Vec<EntryKind> = got.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![EntryKind::Prompt, EntryKind::Thinking, EntryKind::Response, EntryKind::ToolCall],
            "{got:?}"
        );
        assert!(got[0].native_id.as_deref().unwrap().starts_with("prompt-1-"));
        assert_eq!(got[1].native_id.as_deref(), Some("th-1"));
        assert_eq!(got[2].native_id.as_deref(), Some("m-1"));
        assert_eq!(got[3].native_id.as_deref(), Some("call-1"));
        assert_eq!(got[3].tool_name.as_deref(), Some("Read"));
        assert!(got.iter().all(|e| e.turn_seq == 1));
        assert!(got.iter().all(|e| !e.row_id.is_empty()));
    }

    /// A retry rewinds the last turn and re-sends its prompt as a new one. The
    /// rewound turn's rows stay in the store (and the Timeline), but the live
    /// chat no longer shows that exchange, so the anchors the chat pairs with
    /// must leave it out — or every earlier exchange shifts onto it (C1).
    #[test]
    fn a_rewound_turn_is_not_among_the_anchors_and_is_marked_once() {
        use crate::capture::{Capture, SessionKey, TurnContent};
        use crate::model::{ProjectMode, Source, TurnState};

        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join(".atlas")).unwrap();
        let mut capture = Capture::new(&mut store, ProjectMode::Local);
        let key = SessionKey { workspace_id: "ws".into(), source: Source::Native, native_session_id: "native-1".into() };
        let session_id = capture.ensure_session(&key, None, None, None, None).unwrap();
        let reply = |turn: i64, native: &str, body: &str| TurnContent {
            turn_seq: turn,
            native_message_id: Some(native.into()),
            role: Role::Assistant,
            mode: Mode::Text,
            body: body.into(),
            created_at: None,
        };
        for (turn, prompt, native, body) in [(1, "one", "a1", "first"), (2, "two", "a2old", "tried")] {
            capture.record_prompt(&key, prompt, turn, None, None, None).unwrap();
            capture.record_turn(&session_id, reply(turn, native, body)).unwrap();
            capture.finish_turn(&session_id, turn).unwrap();
        }
        // The retry: turn 2 is taken back, and its prompt re-sent as turn 3.
        capture.rewind_turns(&session_id, 1).unwrap();
        capture.record_prompt(&key, "two", 3, None, None, None).unwrap();
        capture.record_turn(&session_id, reply(3, "a2", "retried")).unwrap();
        capture.finish_turn(&session_id, 3).unwrap();

        let turns: Vec<i64> = anchors(&store, &session_id).unwrap().iter().map(|e| e.turn_seq).collect();
        assert_eq!(turns, [1, 1, 3, 3], "turn 2's prompt and reply are gone from the anchors");
        assert_eq!(store.rewound_turns(&session_id).unwrap(), [2].into_iter().collect());
        assert_eq!(store.turn_state(&session_id, 2).unwrap(), Some(TurnState::Rewound));
        assert!(!store.messages_for_session(&session_id).unwrap().is_empty(), "the rows are kept");

        // A second rewind takes the latest live turn, never turn 2 again.
        assert_eq!(store.mark_turns_rewound(&session_id, 1).unwrap(), 1);
        assert_eq!(store.rewound_turns(&session_id).unwrap(), [2, 3].into_iter().collect());
        assert_eq!(store.mark_turns_rewound(&session_id, 0).unwrap(), 0);
    }

    fn entry(kind: EntryKind, turn: i64, at: &str, id: &str) -> TimelineEntry {
        TimelineEntry::blank(id.into(), kind, at.into(), turn)
    }

    #[test]
    fn turn_time_wins_whenever_the_agent_reported_turns() {
        // Turns are agent time by construction, so they win even when they sum
        // to less than the messages' wall gaps.
        assert_eq!(active_seconds(Some((120, 3)), 4_000), 120);
        // A Session whose turns really did take under a second is not the same
        // as one with no turn rows, and must not silently switch clocks.
        assert_eq!(active_seconds(Some((0, 2)), 4_000), 0);
    }

    #[test]
    fn a_session_with_no_turn_rows_falls_back_to_message_gaps() {
        // Every imported transcript — the importer records no turns at all.
        assert_eq!(active_seconds(None, 420), 420);
        assert_eq!(active_seconds(Some((0, 0)), 420), 420);
    }

    #[test]
    fn entries_sort_by_turn_before_time() {
        // A Checkpoint observed long after turn 0 still belongs to turn 0.
        let mut entries = [
            entry(EntryKind::Prompt, 1, "2026-01-01T10:00:00Z", "b"),
            entry(EntryKind::Checkpoint, 0, "2026-01-01T11:00:00Z", "a"),
        ];
        entries.sort_by(order);
        assert_eq!(entries[0].id, "a");
    }

    #[test]
    fn a_checkpoint_closes_the_turn_it_belongs_to() {
        let mut entries = [
            entry(EntryKind::Checkpoint, 2, "2026-01-01T10:00:00Z", "commit"),
            entry(EntryKind::Response, 2, "2026-01-01T10:00:01Z", "said"),
            entry(EntryKind::Prompt, 2, "2026-01-01T10:00:02Z", "asked"),
        ];
        entries.sort_by(order);
        let ids: Vec<_> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["asked", "said", "commit"]);
    }

    #[test]
    fn an_unattributed_checkpoint_sorts_to_the_top_rather_than_vanishing() {
        let mut entries = [
            entry(EntryKind::Prompt, 0, "2026-01-01T10:00:00Z", "first"),
            entry(EntryKind::Checkpoint, -1, "2026-01-01T12:00:00Z", "orphan"),
        ];
        entries.sort_by(order);
        assert_eq!(entries[0].id, "orphan");
    }
}
