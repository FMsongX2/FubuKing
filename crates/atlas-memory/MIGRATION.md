# `atlas-memory` — Migration & Operations

How Atlas's RAG/memory moved from the in-Tauri **brute-force O(n) cosine** over a
flat `memory-index/index.json` to the on-device **MiniLM → usearch HNSW** engine
in this crate (the grafeo graph that briefly sat beside it was removed in #89).
Covers the on-disk layout, the legacy migration, the feature flags, the
retained rollback fallbacks, and the **manual** 3-agent runtime verification.

Background: the originating plan and seam spec lived under the old native-agent SDK path
and were deleted with it (#54). The seam as it stands today is below.

> **The seam.** Every agent retrieves through `memory_retrieve::retrieve(app,
> cwd, query, limit)` in `src-tauri`, which takes **no agent-type parameter**,
> so the path is agent-agnostic by construction. The native agent reaches it
> through its `search_memory` dynamic tool (the `MemorySearch` callback
> installed with `atlas_native_agent::engine::memory::register_search`, returning
> `MemDoc { title, source, text }`); every agent, ACP agents included, also gets
> the pushed `--- RELEVANT PROJECT MEMORY ---` block on send when memory sharing
> is enabled for the project. `atlas-memory` has **no Tauri** dependency and
> depends on no agent crate.

---

## 1. On-disk layout

### Per-project — `<project>/.atlas/memory/`

| Path | Written by | Purpose |
|---|---|---|
| `hnsw.usearch` | `HnswStore::save` | Persistent usearch HNSW index (384-d MiniLM vectors, cosine). |
| `manifest.json` | `Manifest::save` | `{ provider_name, dim, next_key, entries[] }`. Holds the `id ↔ u64 key` bimap and per-doc `content_hash` so an unchanged doc is never re-embedded. **Supersedes** the legacy `index.json`. Atomic write (temp + rename). |
| `docstore.json` | `DocStore::save` | `id → { title, source, text }` side-map so retrieval renders docs without re-gathering the corpus. |
| `extracted/*.md` | `extract.rs` | One markdown file per session of gated native session-extraction output (memdir). Also embedded into HNSW. |
| `memory.sqlite` (+ `-wal`, `-shm`) | `record::RecordStore` | The shared-memory **record store** (#80): `events`, `entries`, `sessions`, WAL. Lives only at the **scope root** (the repository's main worktree, else the launch directory). Replaces `.atlas/shared-memory/events.jsonl` + `state.json` as the Shared tab's store. |
| `.record-store-migrated` | `record::legacy` | Marker: this directory's legacy `shared-memory/events.jsonl` and `extracted/*.md` were folded into its scope's record store. Written in every worktree that had legacy files; the same fact is kept in the store's `legacy_imports` table. The legacy files are kept one release. |

No longer written or read (#89), safe to delete: `graph/` (the grafeo graph;
its content — the legacy shared log — lives in the record store),
`.shared-memory-imported` (its marker), and `.consolidation_lock` /
`.consolidation_state.json` (the dream gates' lock and state; the memdir they
pruned is no longer written, and the record store caps what it shows per kind).

### Global (cross-project) — `~/.atlas/memory/`

| Path | Purpose |
|---|---|
| `MEMORY.md` | Human-readable promoted list, newest first, kept **< 200 lines** (oldest bullets trimmed). |
| `global-promoted.jsonl` | Every promoted memory (`{"content": …}` per line, promotion order, never trimmed) — what global recall searches, together with `MEMORY.md` for promotions older than this file. |
| `global-candidates.json` | Promotion ledger — tracks which content hashes have been seen, in which repositories, and whether they've been promoted. |

Promotion (#89) runs over each repository's record store: a Fact at confidence
≥ 0.8 whose content hash is recorded from ≥ 2 repositories is promoted once.
`global-graph/`, written by older versions, is no longer read. Every promotion
it held was also written to `MEMORY.md`, so recall still finds those older
promotions — except any the list's 200-line cap had already trimmed. Ledger
rows written before #89 (`preference` / `constraint`, keyed by an older hash)
are kept as they are.

The global dir resolves to `~/.atlas/memory/` by default, or the
`ATLAS_GLOBAL_MEMORY_DIR` override (see §3).

---

## 2. Legacy `index.json` (removed in #90)

The flat `<project>/.atlas/memory-index/index.json` is no longer read or
written. Memory ▸ Graph, its natural-language query and the Policy view take
their vectors from this crate's HNSW engine (`MemoryEngine::cached_vector`,
`add_embedded`, `search_ids`), and the one-shot import that used to lift
`index.json` into HNSW on open is gone with it: a project that still has the
file is simply re-embedded by the indexer's first pass. The file (and any
`index.json.bak`) is left on disk untouched.

The legacy `.atlas/shared-memory/events.jsonl` is folded into the record store
by `record::legacy` (guarded by `.record-store-migrated`), not by the engine.

---

## 3. Feature flags & env overrides

| Env var | Default | Effect |
|---|---|---|
| `ATLAS_NATIVE_EXTRACTION` | **OFF** | A/B gate for native session extraction (see below). |
| `ATLAS_GLOBAL_MEMORY_DIR` | unset → `~/.atlas/memory/` | Overrides the global memory dir. Used by tests so they never touch the real home dir. |
| `ATLAS_MINILM_DIR` | unset | Points tests at an installed MiniLM model dir (contains `model.safetensors`). Model-gated tests are `#[ignore = "needs ATLAS_MINILM_DIR"]`; run them with `-- --ignored`. They never download a model. |

### `ATLAS_NATIVE_EXTRACTION` (default OFF) — the A/B plan

Accepted truthy values: `1` / `true` / `on` / `yes` (case-insensitive).

- **OFF (default).** On `TurnFinished`, Atlas runs the legacy
  `memory_compile::compile_finished_turn` per-turn BYOK distill (itself a no-op
  unless the project's summarizer is a BYOK provider). This is the validated
  write-side path.
- **ON.** `TurnFinished` instead enqueues `Job::ExtractSession{cwd, agent, session}`
  into the background `MemoryIndexer` for **every** agent. The gates
  (`should_extract`: ≥20 msgs / ≥3 tool calls / no pending tool_use) decide whether
  to run; on pass, ONE BYOK call (off the hot path) distills the format-neutral
  transcript into `extracted/*.md`, then re-embeds into HNSW.

**A/B plan:** run with the flag ON on a few real sessions per agent, compare the
extracted memories against the `memory_compile` output, and only once the native
path is confirmed at least as good flip it on permanently.

**Deferred `memory_compile` removal:** `memory_compile`'s BYOK round-trip is
**intentionally retained** until the A/B validates the native path. Its removal is
the deferred Step-8 cleanup, gated on that validation — do not delete it as part of
this migration.

---

## 4. What remains for rollback

The pre-HNSW brute-force retrieval (`memory_retrieve::retrieve_brute_force`)
has been deleted; HNSW is the only retrieval path and there is no switch back.
What remains:

- **Archived legacy data** — the original `shared-memory/events.jsonl` (and any
  old `memory-index/index.json[.bak]`) remain on disk, unread.

The micro-benchmark `bench_hnsw_vs_brute_force` (in `atlas-memory`'s
`parity_bench` module, `#[ignore]`d; run with `--ignored --nocapture`) measured
HNSW at roughly **two orders of magnitude** faster per query than a brute-force
cosine on a few-thousand-vector corpus.

---

## 5. MANUAL runtime verification

The offline parity tests (`atlas-memory`'s `parity_bench` module) prove the
retrieval path is agent-agnostic and that `RetrievedDoc` maps cleanly onto
`MemDoc`. They do **not** exercise the live app, real API keys, or the loaded
MiniLM model. That last mile is a **manual** runtime check:

**Prerequisites**
- The MiniLM model installed (so `register_memory_search`'s provider resolves).
- Credentials configured for each agent you test.
- A project with some indexed memory (open it and let the `MemoryIndexer` run, or
  call the `force_reindex` command once).

**Steps — repeat for the native agent and at least one ACP agent**
1. `bun run dev:app` and open the test project, with memory sharing enabled.
2. Confirm the background indexer built the index: `<project>/.atlas/memory/hnsw.usearch`
   and `manifest.json` exist and `manifest.json`'s `entries[]` is non-empty.
3. **Any agent** (push): start a chat turn whose message references known
   project memory (e.g. an established convention). Verify the forwarded prompt
   contains a `--- RELEVANT PROJECT MEMORY ---` block with on-topic snippets.
4. **Native agent** (pull / `search_memory` tool): ask a question that should
   trigger the tool ("what auth strategy does this project use?"). Verify the
   agent invokes `search_memory` and the returned `## title (source)` snippets
   are on-topic.
5. Confirm **identical grounding** across agents — same project + query should
   surface the same underlying docs (the retrieval is shared), differing only in
   push-vs-pull presentation.
6. Flip `ATLAS_NATIVE_EXTRACTION=1`, run a long enough session per agent to pass the
   gates (≥20 msgs / ≥3 tool calls), and confirm `extracted/*.md` appears and the
   new memories become retrievable **without a manual rebuild** (the old
   "invisible until rebuild" bug is gone).

If any agent loses grounding, file the discrepancy before removing any legacy
path in §4.
