//! The MCP surface: seven tools, and the instructions that tell an agent when
//! to call each. Read tools first, write tools last.
//!
//! | tool | answers from |
//! |------|--------------|
//! | `memory_briefing()` | the record (working memory + ranked durable index) and the first-look extras from [`Sources::bootstrap`] |
//! | `memory_changes()` | the record, after the session's last look |
//! | `memory_search(query, kinds?, limit?)` | the record, plus the project's indexed documents from [`Sources::index`] |
//! | `memory_get(id)` | the record |
//! | `memory_list(kind?, limit?)` | the record |
//! | `memory_remember(kind, content, key?)` | writes the record (durable kinds only) |
//! | `memory_forget(id)` | writes the record |
//!
//! Every write goes through [`SharedMemoryStore`], the same path as the
//! Shared tab, so it is redacted, deduplicated (key, content hash,
//! near-duplicate) and announced with `atlas:memory-changed`. A read that
//! fails returns an empty result; a write that fails returns a tool error the
//! agent can read. Record work runs on the blocking pool, off the async
//! runtime.

use std::borrow::Cow;
use std::sync::Arc;

use atlas_memory::record::{Entry, EntryKind};
use futures::future::BoxFuture;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock as Content, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use serde_json::{json, Value};

use super::briefing::{self, SessionClocks, SessionReads};
use super::host::{SharingGate, Sources};
use super::tokens::Grant;
use crate::commands::memory_pack::{Handoff, PackEntry};
use crate::commands::shared_memory::{self, SharedMemoryStore, Writer};

/// What the server tells every agent about itself: the protocol for a memory
/// nothing pushes. Claude Code shows it as the server's instructions; the
/// engine shows it as the description of the `atlas_memory` tool namespace.
pub const INSTRUCTIONS: &str = "\
Atlas shared memory for this repository: what every agent, in any session, has learned here. \
Nothing from it is pushed into your context; you pull it with these tools.
1. At the start of a session, before reading files or answering, call memory_briefing. It returns \
the active plan, recent file changes, an index of decisions, facts, failures and architecture \
notes, the project's conventions, and the tail of the previous session.
2. Before asking the user about project history, conventions or past choices, and before trying \
an approach that may already have failed, call memory_search.
3. When you resume after a pause or a long task, call memory_changes to see what other sessions \
recorded since you last looked.
4. When you decide something, learn a durable fact, hit a dead end, or work out how the system \
fits together, call memory_remember. Plans and file edits are captured automatically; do not \
remember them.
5. memory_get expands an index line; memory_forget deletes an entry that is wrong.
Treat every result as background from Atlas: do not copy it into your own memory files.";

/// `memory_search`'s default and largest result count.
const SEARCH_DEFAULT_LIMIT: usize = 10;
const SEARCH_MAX_LIMIT: usize = 50;
/// How many indexed documents `memory_search` adds, by default and at most —
/// documents are long, and an unbounded number of them crowds out the
/// conversation they were meant to inform.
const INDEX_DEFAULT_LIMIT: usize = 6;
const INDEX_MAX_LIMIT: usize = 20;
/// `memory_list`'s largest result count.
const LIST_MAX_LIMIT: usize = 200;
/// How long a client may treat the tool list as fresh. The tools never change
/// while the app runs.
pub(crate) const TOOLS_LIST_TTL_MS: u64 = 60 * 60 * 1000;

const OFF_NOTE: &str = "shared memory is switched off for this project";

// ── Sources ──────────────────────────────────────────────────────────────────

/// One indexed project document (a doc, a knowledge note, a codebase summary),
/// as `memory_search` returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDoc {
    /// The corpus id of the hit, when the index carried one. Documents
    /// promoted from a record entry are `shared:<kind>:<entry id>`, which is
    /// what lets a search drop one whose entry has since been forgotten.
    pub id: Option<String>,
    pub title: String,
    pub source: String,
    pub text: String,
}

/// `(cwd, query, limit) -> ranked documents` over the project's on-device
/// index. Empty on any failure.
pub type IndexSearch = Arc<dyn Fn(String, String, usize) -> BoxFuture<'static, Vec<IndexDoc>> + Send + Sync>;

/// `(cwd, doc id) -> was it there` — drop one document from the project's
/// index now, rather than at the next whole-corpus pass.
///
/// `memory_forget` needs this: the record delete is immediate, but the index
/// only notices a deletion when a pass re-gathers the corpus and finds the
/// document missing. Without an eviction the forgotten text stays retrievable
/// in the meantime, so `{"forgotten": true}` would not be true yet.
pub type IndexEvict = Arc<dyn Fn(String, String) -> BoxFuture<'static, bool> + Send + Sync>;

/// The first-look extras a briefing carries beyond the record: the curated
/// pack read from the project's foreign stores, and the tail of the most
/// recent other session.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Bootstrap {
    pub project_memory: Vec<PackEntry>,
    pub recent_session: Option<Handoff>,
}

/// `(cwd, session_id) -> extras`. Empty on any failure; time-bounded by the
/// installer.
pub type BootstrapSource = Arc<dyn Fn(String, String) -> BoxFuture<'static, Bootstrap> + Send + Sync>;

// ── Specs ────────────────────────────────────────────────────────────────────

/// The spellings of every kind, or of the durable ones.
fn kind_names(durable_only: bool) -> Vec<&'static str> {
    EntryKind::ALL
        .into_iter()
        .filter(|k| !durable_only || k.is_durable())
        .map(EntryKind::as_str)
        .collect()
}

fn durable_kinds() -> Vec<EntryKind> {
    EntryKind::ALL.into_iter().filter(|k| k.is_durable()).collect()
}

fn schema(value: Value) -> Arc<JsonObject> {
    match value {
        Value::Object(map) => Arc::new(map),
        _ => Arc::new(JsonObject::new()),
    }
}

fn tool(name: &'static str, description: &'static str, input: Value) -> Tool {
    Tool::new(Cow::Borrowed(name), Cow::Borrowed(description), schema(input))
}

/// The tools that READ the record. Reaching for any of them is what makes a
/// session one that consulted memory.
///
/// Declared once and checked against the real tool list by a test, so a tool
/// added later cannot quietly fall out of this set and have the host report
/// that memory went unread when it did not.
pub(super) const READ_TOOLS: [&str; 5] = [
    "memory_briefing",
    "memory_changes",
    "memory_search",
    "memory_get",
    "memory_list",
];

/// The tools, read first, write last.
pub(super) fn tools() -> Vec<Tool> {
    vec![
        tool(
            "memory_briefing",
            "Call this first in a session, before reading files or answering. Returns the repository's \
             shared memory in one read: the active plan and recent file changes (working memory), a \
             ranked index of decisions, facts, failures and architecture notes (`index`, one capped \
             line each; memory_get expands one), the project's conventions from its memory files \
             (`projectMemory`), and the tail of the previous session, whichever agent ran it \
             (`recentSession`).",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "memory_changes",
            "What other sessions recorded since this session last looked (its briefing or its last \
             call here): new or edited entries of every kind, newest first. Call it when resuming \
             after a pause or a long task. Empty when nothing changed.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "memory_search",
            "Search shared memory and the project's indexed documents (docs, conventions, feature \
             notes, codebase summaries). Returns the best matches first: `entries` from shared \
             memory, `documents` from the index. Call it before asking the user about project \
             history or established patterns, and before trying an approach that may have failed. \
             Pass kinds [\"plan\", \"file_changed\"] to search working memory instead.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to look for." },
                    "kinds": { "type": "array", "items": { "type": "string", "enum": kind_names(false) },
                               "description": "Only these kinds (default: the four durable kinds)." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": SEARCH_MAX_LIMIT,
                               "description": "At most this many entries (default 10)." }
                },
                "required": ["query"]
            }),
        ),
        tool(
            "memory_get",
            "One shared-memory entry in full, by its id (from memory_briefing's index, memory_search \
             or memory_list).",
            json!({
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }),
        ),
        tool(
            "memory_list",
            "The newest shared-memory entries, of one kind or of every kind, newest first.",
            json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": kind_names(false) },
                    "limit": { "type": "integer", "minimum": 1, "maximum": LIST_MAX_LIMIT,
                               "description": "At most this many entries (default: each kind's display cap)." }
                }
            }),
        ),
        tool(
            "memory_remember",
            "Record a durable memory for every agent on this repository: a decision (a choice and \
             why), a fact (a project fact or convention), a failure (something tried that did not \
             work) or an architecture note (how the system fits together). Give a key to make a \
             later remember with the same key replace this one. Plans and file changes are captured \
             automatically and cannot be remembered.",
            json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": kind_names(true) },
                    "content": { "type": "string", "description": "The memory, stated on its own." },
                    "key": { "type": "string", "description": "Optional topic key; the same key replaces." }
                },
                "required": ["kind", "content"]
            }),
        ),
        tool(
            "memory_forget",
            "Delete one shared-memory entry that is wrong, by its id.",
            json!({
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }),
        ),
    ]
}

/// The tools' names, in the order they are listed.
#[cfg(test)]
pub(super) fn tool_names() -> Vec<&'static str> {
    ["memory_briefing", "memory_changes", "memory_search", "memory_get", "memory_list", "memory_remember", "memory_forget"]
        .to_vec()
}

/// The `tools/list` answer.
///
/// MCP 2026-07-28 makes `ttlMs` and `cacheScope` required on list results,
/// and rmcp leaves them out unless set. Claude Code negotiates that version
/// and rejects a list without them, so it connected, failed `tools/list`
/// three times and dropped every memory tool. `private`: each answer is
/// served under one session's token.
pub(super) fn tools_list() -> ListToolsResult {
    ListToolsResult::with_all_items(tools())
        .with_ttl_ms(TOOLS_LIST_TTL_MS)
        .with_cache_scope(CacheScope::Private)
}

// ── Arguments ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    kinds: Vec<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct RememberArgs {
    kind: String,
    content: String,
    #[serde(default)]
    key: String,
}

#[derive(Deserialize)]
struct IdArgs {
    id: i64,
}

#[derive(Deserialize)]
struct ListArgs {
    kind: Option<String>,
    limit: Option<usize>,
}

fn parse_kind(raw: &str) -> Result<EntryKind, String> {
    EntryKind::parse(raw.trim()).ok_or_else(|| format!("unknown kind `{raw}`; one of {}", kind_names(false).join(", ")))
}

fn args<T: for<'de> Deserialize<'de>>(request: &CallToolRequestParams) -> Result<T, CallToolResult> {
    let object = request.arguments.clone().unwrap_or_default();
    serde_json::from_value(Value::Object(object)).map_err(|e| tool_error(format!("invalid arguments: {e}")))
}

// ── Results ──────────────────────────────────────────────────────────────────

fn ok_json(value: Value) -> CallToolResult {
    CallToolResult::success(vec![Content::text(value.to_string())])
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(message.into())])
}

fn entries_json(entries: &[Entry]) -> Value {
    json!({ "entries": entries.iter().map(briefing::entry_json).collect::<Vec<_>>() })
}

/// `result`'s JSON object with the index's `documents` added.
fn with_documents(result: CallToolResult, docs: &[IndexDoc]) -> CallToolResult {
    let parsed = result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(Value::Object(mut object)) = parsed else {
        return result;
    };
    let documents: Vec<Value> = docs
        .iter()
        .map(|d| json!({ "title": d.title, "source": d.source, "text": d.text.trim() }))
        .collect();
    object.insert("documents".to_string(), Value::Array(documents));
    ok_json(Value::Object(object))
}

/// The record entry id behind a `shared:<kind>:<entry id>` corpus id.
fn shared_entry_id(doc_id: &str) -> Option<i64> {
    let (_kind, id) = doc_id.strip_prefix("shared:")?.rsplit_once(':')?;
    id.parse().ok()
}

/// Drop documents promoted from record entries that no longer exist.
///
/// Eviction on forget closes the window in the normal case; this closes it
/// again for anything eviction missed — an evict that failed, or a document
/// indexed before the eviction seam existed. It is deliberately narrow: only a
/// document whose id parses as `shared:<kind>:<entry id>` is ever a candidate,
/// and a document is never judged by its text. Anything else passes through
/// untouched, because wrongly dropping a live document would be a worse
/// failure than the one being fixed.
fn live_shared_docs(memory: &SharedMemoryStore, cwd: &str, docs: Vec<IndexDoc>) -> Vec<IndexDoc> {
    docs.into_iter()
        .filter(|doc| match doc.id.as_deref().and_then(shared_entry_id) {
            Some(entry_id) => memory.entry_exists(cwd, entry_id),
            None => true,
        })
        .collect()
}

/// What a tool answers while sharing is off for the project: reads hold
/// nothing, writes are refused.
fn switched_off(name: &str) -> CallToolResult {
    if READ_TOOLS.contains(&name) {
        ok_json(json!({ "entries": [], "note": OFF_NOTE }))
    } else {
        tool_error(OFF_NOTE)
    }
}

// ── The handler ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub(super) struct MemoryTools {
    memory: SharedMemoryStore,
    gate: SharingGate,
    clocks: Arc<SessionClocks>,
    reads: Arc<SessionReads>,
    sources: Sources,
}

impl MemoryTools {
    pub(super) fn new(
        memory: SharedMemoryStore,
        gate: SharingGate,
        clocks: Arc<SessionClocks>,
        reads: Arc<SessionReads>,
        sources: Sources,
    ) -> Self {
        Self { memory, gate, clocks, reads, sources }
    }

    async fn dispatch(&self, grant: Grant, request: CallToolRequestParams) -> CallToolResult {
        let name = request.name.to_string();
        // Recorded BEFORE the sharing gate, and before dispatch. Reaching for
        // memory is what counts as reading it: an agent that called a read
        // tool and got the switched-off note, or an error, still looked. The
        // alternative is telling that session it never consulted memory, which
        // would be a false accusation. Writes are excluded on purpose — an
        // agent that only recorded a fact has not looked at what was there.
        if READ_TOOLS.contains(&name.as_str()) {
            self.reads.read(&grant.session_id);
        }
        if !(self.gate)(&grant.cwd) {
            return switched_off(&name);
        }
        match name.as_str() {
            "memory_briefing" => self.briefing(grant).await,
            "memory_changes" => self.changes(grant).await,
            "memory_search" => self.search(grant, request).await,
            "memory_forget" => self.forget(grant, request).await,
            "memory_get" | "memory_list" | "memory_remember" => {
                let memory = self.memory.clone();
                run_blocking(move || record_call(&memory, &grant, &request))
                    .await
                    .unwrap_or_else(|e| tool_error(format!("memory unavailable: {e}")))
            }
            other => tool_error(format!("unknown tool `{other}`")),
        }
    }

    /// `memory_briefing`: the record's briefing, the first-look extras, and
    /// the session's clock set to what it has now seen.
    async fn briefing(&self, grant: Grant) -> CallToolResult {
        let (cwd, now) = (grant.cwd.clone(), self.memory.now());
        let read = run_blocking(move || {
            shared_memory::store_for(&cwd).and_then(|s| briefing::read_briefing(&s, now).map_err(|e| format!("{e:#}")))
        })
        .await
        .and_then(|read| read);
        let briefing = match read {
            Ok(b) => b,
            Err(e) => return tool_error(format!("memory unavailable: {e}")),
        };
        let mut value = briefing::briefing_json(&briefing);
        if let Some(bootstrap) = &self.sources.bootstrap {
            let extras = bootstrap(grant.cwd.clone(), grant.session_id.clone()).await;
            if !extras.project_memory.is_empty() {
                value["projectMemory"] = json!(extras
                    .project_memory
                    .iter()
                    .map(|p| json!({ "kind": p.kind, "title": p.title, "text": p.text }))
                    .collect::<Vec<_>>());
            }
            if let Some(h) = &extras.recent_session {
                value["recentSession"] = json!({ "text": h.text, "turns": h.turns, "attribution": h.attribution });
            }
        }
        self.clocks.looked(&grant.session_id, briefing.synced_to);
        ok_json(value)
    }

    /// `memory_changes`: what other sessions wrote since this one last looked.
    async fn changes(&self, grant: Grant) -> CallToolResult {
        let since = self.clocks.last_look(&grant.session_id).unwrap_or(0);
        let (cwd, own) = (grant.cwd.clone(), grant.session_id.clone());
        let read = run_blocking(move || {
            shared_memory::store_for(&cwd)
                .and_then(|s| briefing::read_changes(&s, since, &own).map_err(|e| format!("{e:#}")))
        })
        .await
        .and_then(|read| read);
        match read {
            Ok(changes) => {
                self.clocks.looked(&grant.session_id, changes.synced_to);
                ok_json(briefing::changes_json(&changes))
            }
            Err(e) => tool_error(format!("memory unavailable: {e}")),
        }
    }

    /// `memory_forget`: delete the record entry, then evict the document it
    /// was promoted into — and only then report success.
    ///
    /// The old implementation returned `{"forgotten": true}` as soon as the
    /// record row was gone, while the same text stayed retrievable through
    /// `memory_search`'s `documents` until the next whole-corpus pass. A delete
    /// primitive whose own result says the content is gone has to mean it.
    async fn forget(&self, grant: Grant, request: CallToolRequestParams) -> CallToolResult {
        let args: IdArgs = match args(&request) {
            Ok(a) => a,
            Err(refused) => return refused,
        };
        let id = args.id;
        let (memory, cwd) = (self.memory.clone(), grant.cwd.clone());
        let gone = match run_blocking(move || memory.forget(&cwd, id)).await {
            Ok(Ok(gone)) => gone,
            Ok(Err(e)) => return tool_error(format!("not forgotten: {e}")),
            Err(e) => return tool_error(format!("memory unavailable: {e}")),
        };
        let Some(entry) = gone else {
            return ok_json(json!({ "forgotten": false, "id": id }));
        };
        if let Some(evict) = &self.sources.evict {
            let doc_id = crate::commands::agent_memory::shared_doc_id(entry.kind.as_str(), entry.id);
            evict(grant.cwd.clone(), doc_id).await;
        }
        ok_json(json!({ "forgotten": true, "id": id }))
    }

    /// `memory_search` over the record, plus the index when no kinds narrow
    /// the search to working memory.
    async fn search(&self, grant: Grant, request: CallToolRequestParams) -> CallToolResult {
        let args: SearchArgs = match args(&request) {
            Ok(a) => a,
            Err(refused) => return refused,
        };
        let kinds = match args.kinds.iter().map(|k| parse_kind(k)).collect::<Result<Vec<_>, _>>() {
            Ok(k) if k.is_empty() => durable_kinds(),
            Ok(k) => k,
            Err(e) => return tool_error(e),
        };
        let limit = args.limit.unwrap_or(SEARCH_DEFAULT_LIMIT).clamp(1, SEARCH_MAX_LIMIT);
        let (memory, cwd, query) = (self.memory.clone(), grant.cwd.clone(), args.query.clone());
        let hits = run_blocking(move || memory.search_entries(&cwd, &query, &kinds, limit))
            .await
            .unwrap_or_default();
        let result = entries_json(&hits);
        match (&self.sources.index, args.kinds.is_empty()) {
            (Some(index), true) => {
                let limit = args.limit.unwrap_or(INDEX_DEFAULT_LIMIT).clamp(1, INDEX_MAX_LIMIT);
                let docs = index(grant.cwd.clone(), args.query, limit).await;
                // A forgotten entry's document can outlive its record, so the
                // record has the last word on what may be returned.
                //
                // On a pool failure fall back to the UNFILTERED documents, not
                // to none: the per-document check already fails towards
                // keeping, and defaulting to empty here would undo that and
                // drop every live document over an error that has nothing to
                // do with them.
                let (memory, cwd) = (self.memory.clone(), grant.cwd.clone());
                let unfiltered = docs.clone();
                let docs = run_blocking(move || live_shared_docs(&memory, &cwd, docs))
                    .await
                    .unwrap_or(unfiltered);
                with_documents(ok_json(result), &docs)
            }
            _ => ok_json(result),
        }
    }
}

/// The record-only tools. Blocking.
fn record_call(memory: &SharedMemoryStore, grant: &Grant, request: &CallToolRequestParams) -> CallToolResult {
    match request.name.as_ref() {
        "memory_get" => {
            let args: IdArgs = match args(request) {
                Ok(a) => a,
                Err(refused) => return refused,
            };
            match memory.get_entry(&grant.cwd, args.id) {
                Ok(Some(entry)) => ok_json(json!({ "entry": briefing::entry_json(&entry) })),
                Ok(None) => tool_error(format!("no memory entry {}", args.id)),
                Err(e) => tool_error(format!("memory unavailable: {e}")),
            }
        }
        "memory_list" => {
            let args: ListArgs = match args(request) {
                Ok(a) => a,
                Err(refused) => return refused,
            };
            let kind = match args.kind.as_deref().map(parse_kind).transpose() {
                Ok(k) => k,
                Err(e) => return tool_error(e),
            };
            let mut entries = memory.list_entries(&grant.cwd, kind);
            entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
            if let Some(limit) = args.limit {
                entries.truncate(limit.clamp(1, LIST_MAX_LIMIT));
            }
            ok_json(entries_json(&entries))
        }
        "memory_remember" => {
            let args: RememberArgs = match args(request) {
                Ok(a) => a,
                Err(refused) => return refused,
            };
            let kind = match parse_kind(&args.kind) {
                Ok(k) => k,
                Err(e) => return tool_error(e),
            };
            let writer = Writer { agent: grant.agent.clone(), session_id: grant.session_id.clone() };
            match memory.remember(&grant.cwd, &writer, kind, &args.content, &args.key) {
                Ok(r) => ok_json(json!({ "outcome": r.outcome.as_str(), "entry": briefing::entry_json(&r.entry) })),
                Err(e) => tool_error(format!("not remembered: {e}")),
            }
        }
        other => tool_error(format!("unknown tool `{other}`")),
    }
}

/// Run record work on the blocking pool; a pool failure is a readable error.
async fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())
}

impl ServerHandler for MemoryTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(tools_list())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let grant = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<Grant>())
            .cloned()
            .ok_or_else(|| McpError::invalid_request("no session token", None))?;
        Ok(self.dispatch(grant, request).await.into())
    }
}
