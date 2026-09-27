> Historical — written before the rename in ADR-0011 (`vendor/codex` → `vendor/atlas-engine`, `codex-*` crates → `atlas-engine-*`, `"cersei"` → `"atlas-agent"`); paths and names below are as they were.

# How does Atlas's shared memory system work today, and how should it become one memory for every agent?

**Question.** Atlas runs a native agent (the Codex fork, "Atlas Agent") and any
number of ACP agents (Claude Code, Codex CLI, Gemini CLI, …). Each is a
separate process with its own context window. What does Atlas currently do to
give them shared, persistent memory, where does that memory live, who writes
and reads it, and what would a single unified memory look like?

Researched 2026-09-16 on branch `0.3.3` against primary sources only: the
source of `atlas`, the vendored engine at `vendor/codex`, and the official
docs of Claude Code, Gemini CLI and ACP. Every claim carries a `path:line` or
a URL. A live sample of the injected prompt (captured during this session) is
used as evidence in §8. Design decisions were then settled by interview on
2026-09-17 (three rounds, §11–§12) under one constraint: **every capability
the shared memory feature has today survives.** Glossary terms live in
`CONTEXT.md` under "Shared memory domain".

**Status:** research complete, decisions settled, ready for `/to-spec`.
Branch `feat/shared-memory-unification`.

## Outcome in one page

*Read this if you read nothing else. §2–§9 are the evidence; §11–§12 are the
per-question decisions and their citations.*

**What exists today.** Six app-owned stores under `<project>/.atlas/`, three
foreign stores read continuously, one dormant store inside the engine. Memory
reaches every agent by prompt push (four delimited blocks prepended to the
user's text) and reaches the native agent alone by a `search_memory` tool.
ACP agents receive an empty MCP list. Writes come from a keyword pass, a
legacy BYOK distill and a BYOK extractor behind an env flag, so a fresh
install never distills. Injected blocks leak into Claude's private memory
files and are re-injected.

> **Superseded in part (2026-09-21, ADR-0010).** The "Push" row below and
> decisions Q14, Q4 (reopened) and Q15 in §11.2 no longer hold: memory reaches
> an agent only through the tool server, which grew from four tools to seven
> (`memory_briefing`, `memory_changes`, `memory_get` added) and carries the
> read-first protocol in its `instructions`. The rest of this document stands.

**What changes.**

| Area | Decision |
|------|----------|
| Store | One SQLite record store per **repository** (main worktree via git common dir), WAL, backend-only writer, at `<scope root>/.atlas/memory/memory.sqlite`. Tables `entries`, `events`, `sessions`. HNSW vectors stay. The five Shared-tab commands keep their shapes. |
| Kinds | The six existing kinds, unchanged: Active plan, Decision, File changed, Fact, Failure, Architecture. Plan and File changed are **working memory**; the other four are **durable memory**. Session lifecycle is bookkeeping. |
| Reach | One **in-process MCP server over streamable HTTP** on localhost with a per-session bearer token. Offered to an ACP agent when it advertises `mcpCapabilities.http`, and to the native agent through its `mcp_servers` config. Tools: `memory_search`, `memory_remember`, `memory_forget`, `memory_list`. Agents without HTTP keep today's push-only behaviour. |
| Push | All four blocks stay, wrapped in `<atlas-memory>` with a do-not-persist line. Session-start index ranked by recency, use and confidence, capped at today's budgets. Per-turn RAG stays, skipped on short or continuation prompts. Deltas by the existing sync clock. |
| Writers | Delta capture (unchanged), marker phrases (kept as zero-cost fallback), `memory_remember` (new), extractor in BYOK **or gateway** mode (gateway is what makes a fresh install work), user edits, consented imports. Every write passes `atlas-redact`. |
| Ranking and limits | Today's caps become index display limits. Nothing is auto-deleted. Records carry provenance, confidence, `last_used`, `uses`. |
| Pollution loop | Fixed at the reader: `read_claude` strips injected blocks the way capture docs already do. |
| Memory panel | All four tabs stay. Shared gains provenance, edit, forget and live refresh via `atlas:memory-changed`. Import of Claude auto-memory is a previewed, user-triggered action. |
| Global | Promotion to `~/.atlas/memory` stays, rule remapped to high-confidence Facts seen in two or more repositories. Org scope stays out. |
| Deleted | Legacy per-turn distill, grafeo graph paths, legacy flat index, native dynamic tool, stub reader, two dead commands. Each has a named replacement. |

**Order of work.** Twelve tickets (§12.3). The loop fix and the capability
plumbing have no dependencies and ship first; the record store blocks most of
the rest; deletions come last.

**Still to verify at runtime.** Whether the Claude Code and Gemini ACP
adapters advertise HTTP MCP support. Ticket 2 answers it.

---

**Contents.** §1 Summary · §2 Inventory of stores · §3 Data flow · §4
kb-server / company brain · §5 Frontend wiring · §6 Lifecycle and scoping ·
§7 External comparison · §8 Gaps · §9 Open questions (resolved) · §10 Initial
direction (superseded in part) · §11 Round-2 decisions · §12 Round-3
decisions and ticket order.

---

## 1. Summary

- Atlas has **no single memory store**. It has six app-owned stores on disk,
  three foreign stores it reads, and one unused store inside the engine (§2).
- Memory reaches agents by **prompt push** for everyone and by **tool pull**
  for the native agent only. ACP agents get zero tools and an empty MCP list
  (§3). That asymmetry is the core problem.
- The push path prepends up to four delimited blocks to the user's message on
  every turn (`--- SHARED MEMORY ---`, `--- RELEVANT PROJECT MEMORY ---`,
  `--- PROJECT MEMORY ---`, `--- RECENT SESSION ---`) (§3.1).
- Writes come from three uncoordinated mechanisms: a keyword pass over
  deltas, a legacy per-turn BYOK distill, and a gated BYOK session
  extraction behind an env flag that defaults off. **On a fresh install with
  no BYOK key, no LLM ever distills anything** (§6).
- Injected blocks leak back into agents' own memory files and get
  re-injected, which the live sample proves (§8, gap 2).
- The "company brain" of ADR-0006 has no code; `atlas-kb-server` is a static
  export viewer (§4).
- Every external tool solves this with a file convention plus optional MCP;
  ACP itself has no memory primitive, only `mcpServers` on `session/new` (§7).

## 2. Inventory of stores

| # | Store | Owner | Location | Format | Readers | Writers |
|---|-------|-------|----------|--------|---------|---------|
| 1 | **Memory engine** (vectors + graph + memdir) | `crates/atlas-memory` | `<project>/.atlas/memory/` → `manifest.json`, `hnsw.usearch`, `docstore.json`, `graph/` (grafeo), `extracted/<session>.md` (`crates/atlas-memory/src/lib.rs:150-166`, `extract.rs:20-24`) | usearch HNSW + JSON side maps + embedded graph DB + markdown memdir | `memory_retrieve::retrieve` (`src-tauri/src/commands/memory_retrieve.rs:50`), native `search_memory` (`src-tauri/src/commands/agents.rs:809`), Memory ▸ Graph | `MemoryIndexer` jobs `IndexCorpus` / `ExtractSession` / `Compact` (`src-tauri/src/commands/memory_indexer.rs:49-59`) |
| 2 | **Shared cross-agent event log** | `src-tauri/.../shared_memory.rs` | `<project>/.atlas/shared-memory/events.jsonl` + `state.json` (`shared_memory.rs:1-9`) | append-only typed `RawEvent`s (`EventKind`: PlanSet, Decision, FileChanged, Fact, Failure, Architecture, SessionStart/End, TodoAdded/Done; `shared_memory.rs:47-71`) folded into a bounded state view (50/50/50/30/30 caps, `:35-39`) | `memory_inject::build_shared_block`, Memory ▸ Shared tab, `read_shared_memory_docs` (`agent_memory.rs:455`) | `memory_delta::ingest` from every delta (`agents.rs:425-432`), `memory_compile` (BYOK) |
| 3 | **Codebase index** | `codebase_index.rs` | `<project>/.atlas/codebase-index/docs.json` (`codebase_index.rs:1-13`) | per-file structure docs (tree-sitter) + optional LLM summaries | `collect_corpus` (`agent_memory.rs:138` + tail) | `codebase_index_build` command |
| 4 | **Knowledge base notes** | `knowledge.rs` | `<project>/.atlas/knowledge/` | markdown notes, covers, editor state | `read_knowledge_docs` (`agent_memory.rs:388`), Knowledge panel | user via Knowledge panel |
| 5 | **Sharing settings** | `memory_sharing.rs` | `<project>/.atlas/memory-sharing.json`, `memory-summarizer.json` (`memory_sharing.rs:73-78`) | JSON; sharing defaults **on** (`:34`) | `agents_send` | Memory ▸ Sharing controls |
| 6 | **Global memory** | `atlas-memory/src/global.rs` | `~/.atlas/memory/` → `global-graph/`, `MEMORY.md` (<200 lines), `global-candidates.json` (`global.rs:17-25`) | graph + markdown + JSON ledger | `retrieve.rs:283` blend when local memory is sparse | `consolidate.rs:141` promotion (preference/constraint, confidence ≥0.8, seen in ≥2 projects; `global.rs:9-14`) |
| 7 | **Session capture** | `atlas-checkpoint` via `capture.rs` | per-Workspace store (`capture.rs:1-6`) | app-owned transcripts for **every** agent (`agents.rs:283-292`) | `read_capture_docs` (`agent_memory.rs:312`), past-session `@`-mentions, handoff | `OutboundPipeline` stage on every delta |
| 8 | *(legacy, removed in #90)* flat vector index | `memory_graph.rs` | `<project>/.atlas/memory-index/index.json` | JSON blob | was still read by Graph query + Policy until #90, which moved both onto store 1 | was still rewritten by every Graph build until #90; now nothing |
| F1 | *(foreign, read-only)* Claude Code memory | Claude Code | `~/.claude/projects/<encoded-cwd>/memory/*.md` + `MEMORY.md`, `./CLAUDE.md`, `~/.claude/CLAUDE.md` (`agent_memory.rs:4-7`, `collect_corpus`) | markdown with YAML frontmatter | corpus, Memory ▸ Policy (`memory_policy.rs:1-8`) | Claude Code itself |
| F2 | *(foreign, read-only)* Codex CLI | Codex CLI | `~/.codex/state_*.sqlite` threads by cwd, `./AGENTS.md` (`agent_memory.rs:7-10`) | SQLite + markdown | corpus, Memory panel thread list (flagged in `CONTEXT.md:22`) | Codex CLI itself |
| F3 | *(engine, dormant)* Codex-fork memories | `vendor/codex/memories/{read,write}` | `<engine home>/memories` (`vendor/codex/memories/read/src/lib.rs:13-15`) | startup extraction pipeline from rollouts, phase 1/2 prompts (`memories/write/src/lib.rs:1-5`) | the fork, if enabled | the fork, if enabled. Feature key `memories` is `Stable` but `default_enabled: false` (`vendor/codex/features/src/lib.rs:951-955`); `use_memories` defaults true (`vendor/codex/config/src/types.rs:344`) but is ANDed with the feature (`core/src/config/mod.rs:3929`). Atlas's overrides never touch it (`crates/atlas-native-agent/src/engine/config.rs:307-328`) → **off** |

`read_cersei_docs` is a stub returning nothing (`agent_memory.rs:444-446`).

Code volume for the surface: ~5.7k lines of Tauri commands, ~6.0k lines across
`atlas-memory`/`atlas-embed`/`atlas-codeindex`, ~10.5k lines of frontend under
`src/features/memory` and `src/features/knowledge`.

## 3. Data flow per agent kind

```text
 Foreign stores (read-only)                 App-owned stores   <project>/.atlas/
 ┌──────────────────────────┐               ┌─────────────────────────────────────────────┐
 │ ~/.claude/…/memory/*.md  │               │ shared-memory/events.jsonl + state.json      │◄─ memory_delta::ingest
 │ CLAUDE.md (proj + global)│               │                                             │   (plan · file edits · marker phrases,
 │ ~/.codex/state_*.sqlite  │               │                                             │    every delta)
 │ AGENTS.md                │               │                                             │◄─ TurnFinished → memory_compile
 └───────────┬──────────────┘               │                                             │   (BYOK only, default path)
             │                              │ memory/  hnsw.usearch + docstore.json       │◄─ TurnFinished → Job::ExtractSession
             │                              │          + graph/ + extracted/<session>.md  │   (BYOK only, env flag OFF)
             │                              │ codebase-index/docs.json                    │◄─ TurnFinished → enqueue_index
             │                              │ knowledge/   (user notes)                   │
             │                              │ capture      (transcripts, every agent)     │
             │                              └───────────────┬─────────────────────────────┘
             └───────────── collect_corpus ─────────────────┤
                                                            ▼
                                       MemoryEngine  (MiniLM embed → HNSW + graph, RRF fuse)
                                              │                              │
                                 consolidate → promote                  retrieve (RAG)
                                 (preference/constraint,                     │
                                  ≥2 projects)                               │
                                              ▼                              │
                                   ~/.atlas/memory  (global) ─ blend when sparse ─┘

 agents_send   (one path for native AND ACP sessions; bare send if sharing off or slash command)
   ┌ --- SHARED MEMORY ---              delta since this session's sync clock      ◄── events.jsonl
   │ --- RELEVANT PROJECT MEMORY ---    top-3 RAG on the user's text, ≤1400 chars  ◄── MemoryEngine
   │ --- PROJECT MEMORY ---             curated pack, first send only, ≤8000 chars ◄── corpus
   │ --- RECENT SESSION ---             tail of last *Claude* session, first send  ◄── ~/.claude/projects/*.jsonl
   └─► prepended to the user's message text
                    │                                            │
                    ▼                                            ▼
      Native agent (Codex fork)                     ACP agents (Claude Code · Codex · Gemini · …)
      push  +  search_memory dynamic tool           push only  ·  mcpServers = []  ·  no pull path
```

### 3.1 The push path (both kinds)

Native and ACP sessions share one `AgentHost`; the native agent is just the
always-present entry (`src-tauri/src/commands/agent_host.rs:25-48,311-323`).
So `agents_send` applies identically to both (`agents.rs:1240-1425`):

1. Bare send if no cwd or sharing disabled (`:1333-1340`). Slash-command turns
   also ship bare so Claude Code still resolves `/skill` at byte 0
   (`:1349-1360`).
2. `--- SHARED MEMORY ---`: `memory_inject::build_shared_block` gated by a
   per-session sync clock; first sync = full state, later turns = delta only
   (`memory_inject.rs:1-14`, `agents.rs:1366-1369`).
3. `--- RELEVANT PROJECT MEMORY ---`: RAG the engine with the user's text,
   top-3, dedup against docs already injected this session, 320 chars/doc,
   1400 chars total, 6 s hard cap (`memory_retrieve.rs:26-31`,
   `agents.rs:1371-1395`).
4. First send only: `--- PROJECT MEMORY ---` curated pack + `--- RECENT
   SESSION ---` handoff (tail of the most recent *other Claude* session,
   optionally BYOK-summarised), 8 s budget (`memory_pack.rs:1-15`,
   `agents.rs:1229-1231,1397-1405,1431-1490`).
5. All blocks are joined and prepended to the user text (`:1408-1425`).
   Session capture records the *raw* text, not the prefixed one
   (`:1290-1293`); `atlas-agent-transcript` strips `--- X --- … --- END X ---`
   spans when reading transcripts back (`crates/atlas-agent-transcript/src/lib.rs:104-107`).

### 3.2 The pull path (native only)

The fork exposes a `search_memory` dynamic tool declared on `thread/start` and
served back over `item/tool/call`; retrieval is injected as a callback
(`crates/atlas-native-agent/src/engine/memory.rs:1-33`). The app registers it
at `agents.rs:809` over `memory_retrieve::retrieve`. ACP agents get nothing:
`session/new` is sent with `mcp_servers = Vec::new()`
(`crates/atlas-agent-servers/src/connection.rs:854`) and Atlas advertises no
memory tool.

## 4. kb-server / company brain

- ADR-0006 is a one-paragraph intent (consent-based operating record from
  Slack `#team`, Linear, GitHub, PostHog; DMs excluded)
  (`docs/adr/0006-consent-based-company-brain.md`). No code references it.
- `atlas-kb-server` is a single-binary static file server for an **exported**
  knowledge base: embeds HTML at build time, picks a port (default 4747),
  opens a browser (`crates/atlas-kb-server/src/main.rs:1-12`). It is built on
  demand by `knowledge_export.rs:257-303`. It is not a shared brain, has no
  API, no auth, no sync.
- There is no org/user-scoped remote memory anywhere in the app.

## 5. Frontend surfaces and command wiring

Memory panel tabs: `graph`, `policy`, `timeline`, `shared`
(`src/features/memory/components/memory-panel.tsx:36-55`), plus
`memory-sharing-controls.tsx`, `memory-tree-view.tsx`, `provider-pickers.tsx`.
Knowledge is a separate feature (`src/features/knowledge/components/`, 12
files: tree, finder, graph, inspector, editor).

Registered memory-family commands in `src-tauri/src/lib.rs`: 57. Frontend
`invoke(...)` names: 379. Registered but never invoked by the frontend:

| Command | Status |
|---------|--------|
| `import_into_knowledge` | ~~dead~~ **live** — corrected in #90: invoked across lines from `knowledge-panel.tsx` (Import files / folder); kept |
| `knowledge_export_server` | ~~dead~~ **live** — corrected in #90: invoked across lines from `editor-footer.tsx` (Export server); kept |

Everything else is wired. `memory_compile` carries `TODO(step8): remove`
(`agents.rs:456`).

## 6. Lifecycle and scoping

**When memory is written**

| Trigger | Mechanism | Gate | Target |
|---------|-----------|------|--------|
| every delta | `memory_delta::ingest`: `PlanUpdated`, `ToolCallUpserted` file edits, and a marker-phrase pass ("decided to", "note:", "failed:", … `memory_delta.rs:26-30`) | none | events.jsonl |
| `TurnFinished` (default) | `memory_compile::compile_finished_turn` prose→events via BYOK summariser | **no-op unless** project summariser `mode: "provider"` (`memory_compile.rs:11-14`) | events.jsonl |
| `TurnFinished` (`ATLAS_NATIVE_EXTRACTION=1`) | `Job::ExtractSession` → `extract_and_store`: ≥20 turns, ≥3 tool calls since last extraction (`crates/atlas-memory/src/extract.rs:8-12`) | same BYOK gate (`memory_indexer.rs:518-530`) | graph + `extracted/<session>.md` |
| `TurnFinished` (always) | `enqueue_index` re-embeds the corpus (`agents.rs:466-471`); FS watcher on `*.md`/`docs.json` with ~2 s debounce (`memory_indexer.rs:16-19`) | none | HNSW |
| first open, then idle | `Job::Compact` → `consolidate`: prune `extracted/*.md` by confidence floor + cap, promote to global (`consolidate.rs:1-30`) | none | memdir, `~/.atlas/memory` |
| user action | Knowledge notes, Policy edits (rewrite Claude's memory file span in place, `memory_policy.rs:1-6`) | none | knowledge/, foreign files |

**Dedup.** Manifest content-hash per doc for incremental embedding
(`manifest.rs:4-7`); Jaccard near-dup on retrieval (`retrieve.rs:15-16`);
global promotion keyed by `content_hash` (`global.rs:20-23`). No dedup across
events.jsonl vs memdir vs graph.

**Decay/eviction.** Bounded state view caps (§2 row 2); memdir prune by
confidence + newest-first cap. Graph nodes are never deleted because the
grafeo wrapper exposes no delete and returns content only, no id/confidence
(`consolidate.rs:12-21`).

**Scope.** Everything is keyed by the literal `cwd` string
(`MemoryRegistry`: `cwd → Arc<RwLock<MemoryEngine>>`, `memory_indexer.rs:6`).
Two worktrees or a subdirectory launch get separate memory. Global scope is
per-user machine. No org scope.

**Redaction.** `memory_delta::redact` is a local token heuristic
(`looks_secret`, `memory_delta.rs:174-180`); `atlas-redact` is not used on this
path.

## 7. External comparison

| Tool | Mechanism to receive shared context | Own persistent memory | Source |
|------|-------------------------------------|-----------------------|--------|
| Claude Code | `CLAUDE.md` hierarchy: managed → `~/.claude/CLAUDE.md` → `./CLAUDE.md` / `./.claude/CLAUDE.md` → `CLAUDE.local.md`; `@path` imports (depth 4); `.claude/rules/*.md` with `paths:` scoping; subdirectory files load on demand | **Auto memory** at `~/.claude/projects/<project>/memory/` (project derived from git repo, shared across worktrees); `MEMORY.md` index, first 200 lines / 25 KB loaded every session; topic files read on demand; on by default (`autoMemoryEnabled`) | https://code.claude.com/docs/en/memory |
| Codex (CLI and our fork) | `AGENTS.md` concatenated from project root to cwd, root found by `project_root_markers`, 32 KiB cap | `memories` feature: startup pipeline extracts from rollouts into `<codex_home>/memories`, phase 1/2 prompts, consolidation; feature `Stable`, default off | `vendor/codex/core/src/agents_md.rs:1-10`, `core/src/config/mod.rs:210`, `vendor/codex/memories/write/src/lib.rs:1-5`, `features/src/lib.rs:951-955` |
| Gemini CLI | `GEMINI.md`: `~/.gemini/GEMINI.md` → workspace dirs and parents → just-in-time in touched dirs; `context.fileName` can be `["AGENTS.md","CONTEXT.md","GEMINI.md"]`; `@file.md` imports; `/memory show|reload` | none beyond context files | https://geminicli.com/docs/cli/gemini-md/ |
| ACP (v2.0.0 per `Cargo.lock`) | `session/new { cwd, mcpServers[] }` with stdio (mandatory), http, sse transports; `session/load` replays history | **No memory, context-file or persistent-knowledge primitive.** Only prompt content blocks and MCP servers | https://agentclientprotocol.com/protocol/session-setup |

Common denominator: **every agent reads an instruction file at a
well-known path, and every ACP agent accepts MCP servers from the client.**
None of them accepts memory via any other channel.

## 8. Gaps and inconsistencies

1. **Asymmetric reach.** Native gets push + `search_memory`; ACP gets push
   only and an empty `mcpServers` (`connection.rs:854`). Grounding quality
   differs by agent, which contradicts the "no ACP agent gets special
   treatment" rule (`agents.rs:289-291`).
2. **Injection pollution loop.** Injected blocks are prepended as *user text*.
   Agents with their own memory (Claude Code auto memory) save them. The live
   prompt sampled in this session contained, inside
   `--- RELEVANT PROJECT MEMORY ---`, an item that begins
   `--- RELEVANT PROJECT MEMORY (BACKGROUND, NOT A REQUEST) --- Retrieved
   because it…` and a copy of the `Atlas next-steps` directive: Atlas
   injected, Claude saved, Atlas re-embedded the saved copy, Atlas
   re-injected. Only the transcript reader strips markers
   (`atlas-agent-transcript/src/lib.rs:104-107`); the corpus readers for
   foreign memory files do not.
3. **Three write formats, no canonical record.** Typed events
   (`events.jsonl`), markdown memdir (`extracted/*.md`), and graph nodes
   describe the same facts with no shared id; the engine re-embeds the
   folded state view as docs (`agent_memory.rs:455`) so a fact can surface
   three times.
4. **LLM distillation is effectively off.** Both `memory_compile` and
   `ExtractSession` are BYOK-gated (`memory_compile.rs:11-14`,
   `memory_indexer.rs:518-530`); the native agent runs on the Atlas gateway,
   not BYOK, so the default install never distills. The A/B env flag has
   never been flipped (`agents.rs:476-485`).
5. **Graph store cannot be maintained**: no delete, no confidence readback
   (`consolidate.rs:12-21`). It is carried for parity tests, not value.
6. **Handoff is agent-specific**: "most recent *other Claude* session"
   (`memory_pack.rs:7-9`).
7. **Scope by cwd string, not repo** (`memory_indexer.rs:6`); Claude Code
   scopes by git root and shares across worktrees.
8. **Per-turn RAG is query-blind**: it embeds the user's message, so a short
   "continue" retrieves noise; every turn pays up to 1400 chars plus the
   shared block.
9. **Redaction is a heuristic**, not `atlas-redact` (`memory_delta.rs:174`).
10. **Dormant second native memory** in the fork (`memories` feature). If
    anyone flips it, the native agent gets a private store Atlas never sees.
11. **Company brain / kb-server**: ADR only; kb-server is an export viewer.
12. **Dead or legacy**: `import_into_knowledge`, `knowledge_export_server`,
    `read_cersei_docs`, `.atlas/memory-index/` path, `memory_compile`
    (`TODO(step8)`), Codex thread list from `~/.codex/state_*.sqlite`
    (flagged in `CONTEXT.md:22`).

## 9. Open questions (all resolved 2026-09-17)

- Should Atlas keep reading foreign stores (Claude memory dir, Codex SQLite)
  at all once it has a canonical store, or only import them once?
  **Resolved: keep reading; fix the loop at the reader; add an optional
  consented import** (§11.2 Q7, Q24).
- Is the gateway model allowed to run background extraction (cost/entitlement
  per ADR-0007)? **Resolved: yes, gateway becomes the default extraction mode
  when signed in, BYOK stays as an alternative; entitlement is checked the
  same way the native agent's turns are** (§11.2 Q6).
- Does the product want an org scope (ADR-0006) before or after the local
  unification? **Resolved: after. The record table carries no org field yet;
  org becomes an import source when it arrives** (§11.2 Q20, round 1 Q10).

## 10. Initial proposed direction (2026-09-16, superseded in part)

*Kept for the record of how the design moved. Where this section and §11–§12
disagree, §11–§12 win. The points that were reversed by the "no capability
lost" constraint or by code evidence: the server runs **in-process over
HTTP**, not as a stdio child; per-turn RAG is **kept**; foreign stores are
**still read** (with stripping) rather than imported once; the marker-phrase
capture, the Codex thread list, the Policy view and the session handoff all
**stay**; the file projection to `.atlas/MEMORY.md` was **not adopted**
(nothing in Atlas needs it while the tag and the tools exist).*

One store, one reach mechanism, one write discipline.

- **One store per repo** (git common dir, not cwd): a single typed table of
  memory records `{id, kind, content, source_agent, session, confidence,
  created, last_used, scope, content_hash}` plus the existing HNSW for
  vectors. Retire `events.jsonl`, the graph, and the memdir as separate
  sources; keep capture as the raw transcript archive.
- **One reach mechanism for every agent: an Atlas MCP memory server**
  (stdio) passed on `session/new mcpServers` to every ACP agent, and the same
  tools registered as dynamic tools on the native engine. Tools:
  `memory_search`, `memory_remember`, `memory_forget`, `memory_list`. This is
  the only channel ACP defines, and it gives ACP agents the pull path they
  lack today.
- **Slim, marked push**: at session start inject a ≤200-line `MEMORY.md`-style
  index (matching what Claude Code and Codex already do with files), then
  only deltas of new high-confidence records. Wrap blocks in a
  `<atlas-context>` tag with an explicit "do not persist" line and strip the
  tag in every corpus reader so the loop in gap 2 cannot recur.
- **Also project the index to disk** at `<repo>/.atlas/MEMORY.md` and offer a
  one-line `@.atlas/MEMORY.md` import for `CLAUDE.md` / `AGENTS.md` /
  `GEMINI.md`, so agents that read files get it for free even when launched
  outside Atlas.
- **Writes**: agent-explicit via `memory_remember` first; background
  extraction on `TurnFinished` through the gateway model (with the existing
  ≥20-turn / ≥3-tool gates) so it works on a fresh install; keep the
  structured delta capture for plans and file changes only; drop the
  marker-phrase heuristic.
- **Lifecycle**: `content_hash` dedup on write, `last_used` bump on
  retrieval, single `Compact` job doing decay + promotion to global; org
  scope (ADR-0006) becomes an import source later, not a fourth store.
- **Delete**: `memory_compile`, `memory_graph`'s legacy index, the grafeo
  graph, `read_cersei_docs`, the two dead commands, the Codex SQLite thread
  list, and the Claude-only handoff.

### 10.1 Target-state diagram (final, reflects §11–§12)

```text
                     MEMORY PANEL   Shared (provenance · edit · forget · live) · Graph · Tree · Policy · Timeline
                                    ▲                                      │
                 five existing commands + atlas:memory-changed event        │ user edits · consented import
                                    │                                      ▼
 ┌──────────────────────────────────┴──────────────────────────────────────────────────────────────┐
 │  ATLAS MEMORY STORE   one per repository (main worktree via git common dir) · backend-only writer│
 │                                                                                                 │
 │  <scope root>/.atlas/memory/memory.sqlite (WAL)    entries · events · sessions                  │
 │  hnsw.usearch                                       vectors, on-device MiniLM, keyed by entry id │
 │  capture                                            raw transcripts, every agent                 │
 │                                                                                                 │
 │  entries: id · kind · key · content · source · agent · session · confidence ·                    │
 │           created_at · updated_at · last_used_at · uses · content_hash                          │
 │  caps = index display limits · nothing auto-deleted · every write through atlas-redact          │
 └───────▲──────────▲──────────────▲──────────────▲───────────────────────────┬────────────────────┘
         │          │              │              │                           │ Facts ≥0.8 in ≥2 repos
   delta capture  memory_remember  extractor      read-only sources           ▼
   plan · files   (tool, new)      turn-finished  CLAUDE.md · AGENTS.md    ~/.atlas/memory (global)
   marker phrases                  session-end    Claude memory dir
   (unchanged)                     gateway | BYOK Codex threads · Knowledge · Codebase index
                                                  (atlas-memory tag stripped at the reader)

 ┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
 │  REACH                                                                                          │
 │                                                                                                 │
 │  A. In-process MCP server · streamable HTTP on 127.0.0.1 · per-session bearer token             │
 │     tools: memory_search · memory_remember · memory_forget · memory_list                        │
 │       ├─► ACP agents      session/new mcpServers   — only when agent advertises mcpCapabilities.http
 │       └─► Native agent    mcp_servers.atlas_memory (StreamableHttp) config override             │
 │                                                                                                 │
 │  B. Tagged push  <atlas-memory> … </atlas-memory>  with a do-not-persist line                   │
 │     first send : working memory + ranked durable index (recency · uses · confidence,            │
 │                  today's caps and char budgets) + pack + agent-neutral handoff                   │
 │     every turn : RELEVANT PROJECT MEMORY (RAG, skipped on short/continuation prompts)           │
 │                  + SHARED MEMORY delta by sync clock                                             │
 └───────────────┬────────────────────────────────────────────┬────────────────────────────────────┘
                 ▼                                            ▼
   Native agent (Codex fork)                     ACP agents (Claude Code · Codex · Gemini · …)
   tools + push                                  tools + push  — or push only, exactly as today,
                                                 when the agent has no HTTP MCP capability
```

Versus today, four things change and nothing is removed: every agent that
can speak HTTP MCP gets the same pull and explicit-write tools; injected
context is tagged and stripped so it cannot be re-absorbed; all writers land
in one record table with provenance and confidence; and extraction runs on a
fresh install through the gateway.

---

## 11. Round-2 decisions, chosen from Atlas evidence, with today's capabilities preserved

Decided 2026-09-17 under one constraint from the user: **no capability the
shared memory feature has today may be lost.** Each answer names the evidence
that picked it. Where this reverses an earlier recommendation, it says so.

### 11.1 Facts that changed the answers

| Fact | Where | Consequence |
|------|-------|-------------|
| The Atlas binary deliberately does **not** dispatch on arg0: "`current_exe()` is a helper only in a process that dispatches on arg0, and Atlas does not" | `crates/atlas-native-agent/src/engine/config.rs:228-230` | Re-executing Atlas as a stdio MCP child would overturn a stated design choice. Rejected. |
| No sidecar binaries exist (`externalBin` absent in `src-tauri/tauri.conf.json`); CI builds macOS + Ubuntu, Windows landed 2026-09 | `tauri.conf.json`, `.github/workflows/ci.yml` | A sidecar is a new sign/notarise/CI surface on three OSes. Not first choice. |
| `rmcp` 3.0 with the `server` feature is already compiled through the fork; `axum` 0.8 is already a workspace dependency | `vendor/codex/codex-mcp/Cargo.toml:35`, `Cargo.toml:357,479` | An in-process MCP server costs no new dependency. |
| ACP: stdio MCP is the mandatory transport; HTTP is optional and advertised per agent as `agentCapabilities.mcpCapabilities.http`; Atlas already stores the agent's capabilities after `initialize` | https://agentclientprotocol.com/protocol/session-setup, https://agentclientprotocol.com/protocol/initialization, `crates/atlas-agent-servers/src/connection.rs:116,378` | Atlas can decide per agent, at runtime, whether to offer the server. |
| The fork's MCP client config supports `StreamableHttp` natively and Atlas already writes dotted `harness_overrides` into it | `vendor/codex/config/src/mcp_types.rs:514-528`, `config.rs:307-328`, `vendor/codex/config/src/overrides.rs:18-22` | The native agent can consume the same HTTP server through config, no dynamic tool needed. |
| The shared-memory store's stated invariant is **one backend writer, in-process mutex, no cross-process locking** | `src-tauri/src/commands/shared_memory.rs:11-14` | An in-process server keeps that invariant; a child process would break it. |
| The thread-metadata SQLite store already runs in WAL mode | `crates/atlas-thread-metadata/src/db.rs:60-62` | SQLite precedent exists if a second process ever needs the file. |
| The pollution loop has a single cause: Claude's memory files are read into the corpus **without** `strip_injected_context`, while capture docs are stripped | `src-tauri/src/commands/agent_memory.rs:313` vs `read_claude` (no strip) | The loop is fixed by stripping at that reader. Stopping the reads is unnecessary. |
| The session handoff reads `~/.claude/projects/*.jsonl` directly, so it only works when the previous agent was Claude | `src-tauri/src/commands/memory_pack.rs:100-109` | Capture already holds every agent's transcript (`agent_memory.rs:312`); the handoff can read capture and become agent-neutral. |
| Summariser modes are `raw`, `provider`, and a `local` mode that is documented as a future phase and never implemented | `memory_sharing.rs:39-41`, `memory_summarize.rs:3` | The gateway becomes the third real mode; the UI type already has a slot. |

### 11.2 Answers

| Q | Decision | Evidence / reason | Change vs earlier |
|---|----------|-------------------|-------------------|
| Q12 server process | **In-process MCP server over streamable HTTP on `127.0.0.1`, random port, per-session bearer token**, hosted by the Tauri backend with `rmcp` `server` + `axum`. Offered to an ACP agent only when its `initialize` response advertises `mcpCapabilities.http`; offered to the native agent through a `mcp_servers.atlas_memory` StreamableHttp override. Agents without HTTP keep today's push-only behaviour. A stdio bridge is a follow-up ticket if a registry agent needs one. | keeps the one-writer invariant, no arg0 dispatch, no sidecar, MiniLM stays loaded once, ADR-0002 rule of "gate on advertised capabilities" | **Reversed** from stdio self-exec |
| Q13 tools | `memory_search(query, kinds?, limit?)`, `memory_remember(kind, content, key?)` for the durable four, `memory_forget(id)`, `memory_list(kind?)`. Working memory stays delta-captured. | plan and file edits already arrive structured (`memory_delta.rs:8-11`) | unchanged |
| Q14 session-start block | Working memory + durable index (≤200 lines) + one tool-hint line, wrapped in `<atlas-memory>` … `</atlas-memory>`; **also keep the `--- PROJECT MEMORY ---` pack and `--- RECENT SESSION ---` handoff** inside the tag | pack + handoff are current first-send capabilities | modified: pack/handoff kept |
| Q4 (reopened) per-turn RAG | **Keep** `--- RELEVANT PROJECT MEMORY ---`, wrapped in the tag, skipped when the prompt is under a short-length floor, dedup by the existing `note_index_doc` clock | it is the only grounding for agents that never call a tool | **Reversed** from "retire" |
| Q15 delta block | Other-sessions' new durable entries + working-memory changes, by the existing per-session sync clock | `memory_inject.rs:4-11` already implements it | unchanged |
| Q16 replace/dedup | Agent key, else normalised content hash; near-duplicate merge at ~0.92 cosine | preserves "same key replaces" (`shared_memory.rs`) and closes triple-surfacing | unchanged |
| Q17 caps | Caps become **index** limits; storage keeps everything searchable; nothing auto-deleted | user distrust of silent loss | unchanged |
| Q18 confidence | Extractor 0–1; tool/user 1.0; import keeps source or 0.7 | ranking needs it | unchanged |
| Q19 redaction | `atlas_redact::redact_auto` on every write | crate exists (`crates/atlas-redact/src/lib.rs:222`) | unchanged |
| Q20 global promotion | **Keep** promotion; rule becomes: `Fact` entries with confidence ≥0.8 seen in ≥2 repositories are promoted to `~/.atlas/memory` | current `global.rs` behaviour survives with kinds mapped | **Reversed** from "drop" |
| Q6 (reopened) extraction model | `provider` (BYOK) stays; **gateway** becomes the default when signed in; `raw` unchanged. `local` slot reserved. | fresh install must distill; UI type already has three modes | modified |
| Q7 (reopened) foreign stores | **Keep continuous reads** of Claude memory dir, CLAUDE.md, AGENTS.md, Codex SQLite; apply `strip_injected_context` in `read_claude`; **add** an optional one-time import of Claude auto-memory into durable memory | Policy, Graph and Tree views depend on the reads; the loop is fixed at the reader | **Reversed** from "import once" |
| Q21 Memory panel | **All four tabs stay** (Graph, Policy, Timeline, Shared). Shared gains provenance, edit and forget. Tree view stays. | every tab is a shipped capability | **Reversed** from "retire Graph/Policy" |
| Q22 toggle | One per-project switch, default on, gates server, block, extractor, writers | `DEFAULT_ENABLED = true` (`memory_sharing.rs:34`) | unchanged |
| Q23 cadence | Turn-finished with existing gates + once at session end | `extract.rs:8-12` | unchanged |
| Q24 import | User-triggered, with preview, once per source | consent; files another program wrote | unchanged (now additive to Q7) |
| Q25 deletions | **Trimmed.** Go: legacy per-turn distill (`memory_compile`, superseded by extractor with the same BYOK gate plus gateway), grafeo graph + `consolidate`/`dream` graph paths (retrieval keeps HNSW; promotion re-implemented on the record table), legacy `.atlas/memory-index`, native `search_memory` dynamic tool (replaced by the MCP tool), `read_cersei_docs` stub, the two dead knowledge commands. **Stay:** marker-phrase capture (zero-cost fallback writer), Codex SQLite thread list, Policy view, session handoff (made agent-neutral via capture). | each deleted item has a named replacement carrying the same capability | **Trimmed** |

### 11.3 Capability-preservation matrix

| Capability today | Where it lives after |
|------------------|----------------------|
| Six kinds captured from every agent with no agent cooperation | same `memory_delta` capture → record table |
| Bounded "current truth" view (plan, 50/50/50/30/30) | index view over the record table, same caps as display limits |
| Per-turn shared block with sync clock | unchanged, inside `<atlas-memory>` |
| Per-turn RAG block | unchanged, inside the tag, short-prompt skip |
| First-send pack + recent-session handoff | unchanged, handoff now reads capture for any agent |
| BYOK summariser / distill | extractor keeps BYOK mode, adds gateway |
| Native `search_memory` | `memory_search` over MCP |
| Memory ▸ Shared: state, events, query, clear, append | same five commands, same response shapes, backed by SQLite |
| Memory ▸ Graph / Tree: embed model download, index build, NL query | unchanged (`memory_graph.rs`, HNSW) |
| Memory ▸ Policy: probe table over Claude memory + CLAUDE.md + AGENTS.md, in-place edit | unchanged |
| Memory ▸ Timeline: git + sessions + memory events | unchanged, reads record table for events |
| Sharing toggle + summariser prefs | unchanged files, one more mode |
| Global promotion to `~/.atlas/memory` | kept, rule mapped to `Fact` |
| Knowledge notes retrievable by agents | unchanged (`read_knowledge_docs`) |

### 11.4 What is new, not replaced

ACP agents gain a pull path (the MCP tools). Every agent gains an explicit
write path (`memory_remember`). Records gain provenance, confidence and
`last_used`. Injected context stops leaking into agents' private memory.

## 12. Round-3 decisions (the questions round 2 unblocked)

Same method: chosen from Atlas evidence, no capability lost.

### 12.1 Facts found for this round

| Fact | Where | Consequence |
|------|-------|-------------|
| `SessionEnd` exists as an event kind but is **never recorded** in production; the only occurrence is a unit test. `SessionStart` is implicit in `register_session` (owner binding) | `shared_memory.rs:57-58,756`, `agents.rs:1363` | "tracks session end" is aspiration, not behaviour. Round 3 defines and records it. |
| The Memory ▸ Shared store refreshes only on demand (`refresh()`); no Tauri event exists for memory changes (the only memory-family event is `atlas:models-changed`) | `src/features/memory/stores/shared-memory-store.ts:32,63` | Tool writes from agents would be invisible until a manual refresh. |
| `agent_capabilities` is held by the ACP connection but not plumbed to `atlas-agent-manager` or the Tauri layer | `crates/atlas-agent-servers/src/connection.rs:116,378`; no hits in `atlas-agent-manager`/`src-tauri` | The HTTP-capability gate needs a small plumbing ticket first. |
| No helper resolves the git common dir anywhere in Atlas; `atlas-checkpoint` has a generic `run(repo, args)` git helper; the thread-metadata store already models `main_worktree_paths` | `crates/atlas-checkpoint/src/git.rs:220-226`, `crates/atlas-thread-metadata/src/db.rs:40-42` | Repo scope is one new helper; worktree awareness has precedent. |
| `.atlas/` is gitignored | `.gitignore:26` | The store is machine-local by default, like Claude's auto-memory. |
| First-send budget today: pack ≤ 8000 chars, 400/entry, handoff ≤ 8 turns × 800 chars; per-turn RAG ≤ 1400 chars, 320/doc | `memory_pack.rs:30-39`, `memory_retrieve.rs:29-31` | The new index inherits these ceilings so first-send cost does not grow. |
| The one-time import pattern with a marker file already exists | `crates/atlas-memory/src/shared_import.rs:18-37` | Migration reuses it. |

### 12.2 Answers

| # | Decision | Evidence / reason |
|---|----------|-------------------|
| R1 record store | SQLite `memory.sqlite` at `<scope root>/.atlas/memory/`, WAL, opened by the backend only. Tables: `entries(id, kind, key, content, source, agent, session, confidence, created_at, updated_at, last_used_at, uses, content_hash)`, `events(seq, ts, kind, key, agent, session, payload)` (keeps the events list/append/query API byte-compatible), `sessions(session_id, agent, started_at, ended_at)`. Vectors stay in the HNSW keyed by entry id. | thread-metadata WAL precedent; five existing commands keep their response shapes |
| R2 scope root | Parent of `git rev-parse --git-common-dir` (the main worktree), else the launch directory. New helper beside the existing git runner. Existing per-worktree `.atlas/memory` dirs are migrated into the main worktree's on first open. | `.gitignore:26`, `git.rs:220-226`, thread-metadata worktree columns |
| R3 index ranking | Working memory first (plan, then files changed newest-first). Durable entries scored `recency(half-life 14 d) + ln(1+uses) + confidence`, grouped by kind with today's caps as display limits. Whole block ≤ 200 lines and ≤ 8000 chars. | reuses `PACK_MAX_CHARS`; caps preserved as limits |
| R4 RAG floor | Skip per-turn retrieval when the prompt has fewer than three words or is a continuation phrase ("continue", "ok", "yes", "go on", "next"). | live sample: "continue" retrieved noise |
| R5 session end | Defined as `agents_drop_session` or agent process exit. Record `SessionEnd`, set `sessions.ended_at`, enqueue the end-of-session extraction. | never recorded today; round 2 Q23 depends on it |
| R6 UI refresh | The in-process server and every writer emit `atlas:memory-changed { scopeRoot, kinds }`; the Shared store subscribes and re-pulls. Manual refresh stays. | store is pull-only today |
| R7 capability plumbing | Expose `agent_capabilities` through `atlas-agent-manager` to the Tauri layer. On `session/new`, include the memory server only when `mcpCapabilities.http` is true. Log the decision per agent so the first runtime check answers which adapters qualify. | capabilities stored but unplumbed |
| R8 server identity | One server per app on `127.0.0.1:0`; one bearer token per (session, scope) minted at `session/new`, passed in the MCP `headers` array for ACP and in the fork's StreamableHttp header field. Token revoked on session end. | ACP http entry carries `headers`; fork supports StreamableHttp |
| R9 migration | On first open per scope: `events.jsonl` → `events` + folded `entries`; `extracted/*.md` → `entries` (category → kind: decision→Decision, failure→Failure, architecture→Architecture, everything else→Fact); legacy `memory-index` dropped. Marker file gates re-runs; old files kept one release. | `shared_import.rs` pattern |
| R10 Claude import mapping | Frontmatter `type` → kind: `feedback`→Fact, `reference`→Fact, `user`→Fact, `project`→Decision when the body states a choice, else Fact. Confidence 0.7, source `import:claude`. Preview lists each mapped line before write. | Claude docs define the four types |
| R11 extractor output | The extraction prompt asks directly for the four durable kinds plus a 0–1 confidence; no intermediate category table. | avoids the lossy category→type mapping the graph path had (`extract.rs:14-17`) |
| R12 tests | Keep the existing shape tests on `memory_get_state`/`memory_list_events`. Add: rmcp in-process client tests for the four tools; a strip-at-reader test that feeds a Claude memory file containing an `<atlas-memory>` block and asserts nothing is embedded; a scope test with two worktrees resolving to one store. | contract preservation is the constraint |

### 12.3 Ticket order (blocking edges)

1. **Loop fix** — `<atlas-memory>` tag on every injected block; strip in `read_claude` and the transcript reader. No dependencies. Ships alone.
2. **Capability plumbing** — expose `agent_capabilities`; log `mcpCapabilities.http` per agent at runtime. No dependencies.
3. **Record store + migration** — SQLite behind the five existing commands; `sessions` table; `SessionEnd` recorded. Blocks 4, 6, 7, 9, 10.
4. **In-process MCP server** — rmcp over axum, four tools, token minting, `atlas:memory-changed`. Blocks 5.
5. **Wiring** — ACP `session/new` gated on (2); native `mcp_servers.atlas_memory` override; delete the dynamic tool. Needs 2, 4.
6. **Session-start block** — index ranking, RAG floor, pack + handoff inside the tag. Needs 3.
7. **Extractor** — gateway mode + BYOK mode, direct four-kind output, end-of-session trigger; delete `memory_compile`. Needs 3.
8. **Handoff via capture** — agent-neutral recent-session tail. Needs 3.
9. **Memory panel** — provenance, edit, forget, live refresh. Needs 3, 4.
10. **Import** — Claude auto-memory, previewed. Needs 3, 9.
11. **Global promotion remap** — Fact rule over the record table; delete graph/consolidate/dream. Needs 3, 7.
12. **Deletions** — legacy index, stub reader, dead commands. Needs 5, 7, 11.
