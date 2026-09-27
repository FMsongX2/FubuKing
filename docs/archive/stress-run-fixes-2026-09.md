> Historical — written before the rename in ADR-0011 (`vendor/codex` → `vendor/atlas-engine`, `codex-*` crates → `atlas-engine-*`, `"cersei"` → `"atlas-agent"`); paths and names below are as they were.

# Stress-run fixes — the five open issues from 2026-09-21, root causes and a step-by-step plan

*Issues `#294`, `#292`, `#291`, `#289`, `#286` on `pacifio/atlas`, all filed by pacifio from a Windows 11 dev build of `0.3.3` @ `69c56bfd`. PR #295 (merged 2026-09-22) closed the dialog-title half of #294 plus #288 and #293; this document covers everything #295 deliberately left open.*

Each issue thread already holds a correct partial analysis and stops at "needs a decision". This document makes those decisions, verified against the code as of `0f64941f`, and orders the work so each step ships and is tested on its own. Line numbers are as of that commit.

## What the research changed

The threads' own conclusions are right about the symptoms and wrong, in four places, about how deep the fix has to go:

| Issue | Thread's conclusion | What the code says |
|---|---|---|
| #294 | Windows agent execution is unsandboxed; enabling the sandbox is a risky first-time flip | Two separate things, and only one of them is the bug. **File writes on Windows are already sandboxed**: `apply_patch`'s write context upgrades `Disabled → RestrictedToken` for a Windows cwd (`vendor/codex/core/src/tools/runtimes/apply_patch.rs:98-107` via `tools/sandboxing.rs:404-415`), so the write really does go through a restricted-token helper process. Only `assess_patch_safety` is handed the raw `Disabled` level (`vendor/codex/core/src/apply_patch.rs:29-34`), which is the whole bug. **Shell is a different story and is NOT sandboxed today**: the exec path selects from the raw level (`tools/orchestrator.rs:244-249`, `sandboxing/src/manager.rs:285-298`), so it resolves to `SandboxType::None`. The one-line consistency patch closes #294 on its own; the config key is a separate posture change that would bring shell under the token for the first time. |
| #292 | A ~2-minute eviction lag; needs an eviction hook that does not exist | There is no timer. The doc index is rebuilt only by a full `IndexCorpus` pass; the UI forget command nudges one (`src-tauri/src/commands/shared_memory.rs:976`), the MCP `memory_forget` tool does not (`src-tauri/src/commands/memory_server/tools.rs:479-488`). A per-document delete already exists inside `MemoryEngine::index_corpus` (`crates/atlas-memory/src/lib.rs:302-307`); it is just not exposed. |
| #289 | Needs a new persisted turn-terminal state; a schema addition | No new persistence. The engine's `thread/resume` response already carries `TurnStatus { Completed, Interrupted, Failed, InProgress }`, `TurnError` and every tool-call item (`vendor/codex/app-server-protocol/src/protocol/v2/thread_data.rs:269-289`). Atlas discards all of it in one 90-line file, `crates/atlas-native-agent/src/engine/replay.rs`. `atlas-checkpoint` already has `TurnState::Aborted` reconciliation; it is the Memory store, not the transcript source. |
| #286 | A product decision that reverses ADR-0010 | MCP tool deferral is per-server configurable (`mcp_servers.<name>.omit_tools_from = ["deferred"]`, `vendor/codex/core/src/tools/spec_plan.rs:192-198`) through the per-thread override map Atlas already builds (`crates/atlas-native-agent/src/engine/mcp.rs:21-40`). `ToolSearchAlwaysDeferMcpTools` is a removed no-op feature, so the flag cannot be flipped; the exemption is the lever. `developer_instructions` on `ThreadStartParams` is empty today and is an engine-side carrier that never touches the user's message. Both refine ADR-0010 rather than reverse it. |
| #291 | Gateway-side; unfixable from the client | Confirmed, and now precise: the gateway's Anthropic translation (`apps/ai/src/anthropic.ts` in the server repo) reads `cache_read_input_tokens` from the reply (l.464-465 in the reference copy) but never adds `cache_control` breakpoints to the request, so Anthropic never caches. The client cannot add them: the request contract is an 8-key allowlist and nested unknowns are a 400. |

**Order of execution:** Step 1 (#294) → Step 2 (#289) → Step 3 (#292) → Step 4 (#286) → Step 5 (#291). Steps 1–3 are pure bug fixes. Step 4 amends ADR-0010's consequences. Step 5 is mostly a gateway ticket with three small client-side items.

---

## Decisions taken 2026-09-22

Settled by the repo owner in a grilling round after the research above. Three of these were flagged by pacifio as not his to pick.

| # | Decision | Consequence |
|---|---|---|
| D1 | **#294 ships as the consistency patch alone.** The `windows.sandbox` config key is not part of the fix. | Step 1 change 1 moves out of the #294 ticket. Change 2 is the fix. |
| D2 | **Turning the Windows sandbox on is a separate, held ticket.** It does not merge until a real command matrix has been run on Windows. | Tracked on the fork; a comment goes on #294 upstream because pacifio has the harness and offered to run it. Which level to use, unelevated or elevated, is decided inside that ticket with the matrix results in hand. |
| D3 | **Resume restores the last explicitly picked approval mode, Bypass included.** | Read from the existing global per-agent-type preference. No new per-thread storage. |
| D4 | **The mode picker and the engine must never disagree.** | Fixed in the same ticket as D3, at both resume call sites. |
| D5 | **Ship the tool-visibility fix and the not-consulted notice. Hold the instruction-text nudge.** | Step 4 changes 1, 3 and 4 ship. Change 2 is held, and its recommendation is posted on #286 before anything touching the ADR is built. |
| D6 | **ADR-0010 is not amended.** | Nothing shipping changes what it decided, so the amendment proved unnecessary rather than contested. |
| D7 | **The escalated-write finding is filed upstream as its own issue, not fixed here.** | See "Open items" at the end. |

**One combination, recorded as deliberate.** D3 restores Bypass while D1/D2 leave the Windows sandbox off. The first shell command after a crash therefore runs at full user privilege with no prompt on Windows. This was chosen with the coupling stated.

---

## Step 1 · #294 — Accept-edits must auto-approve file edits on Windows

### Root cause

`crates/atlas-native-agent/src/engine/modes.rs:105` maps `acceptEdits → (AskForApproval::OnRequest, workspace_write())`. Under `OnRequest`, `assess_patch_safety` (`vendor/codex/core/src/safety.rs:72`) auto-approves only when `get_platform_sandbox(windows_sandbox_level != Disabled)` is `Some`:

```rust
match get_platform_sandbox(windows_sandbox_level != WindowsSandboxLevel::Disabled) {
    Some(_) => SafetyCheck::AutoApprove,
    None    => SafetyCheck::AskUser,
}
```

Atlas never writes the `windows.sandbox` TOML key. `EngineSettings::cli_overrides()` in `crates/atlas-native-agent/src/engine/config.rs` (~l.302-360) writes `model_providers.*`, `analytics.enabled` and `model_catalog_json` and nothing under `windows.*`, so the level is `Disabled` on Windows and every edit prompts. macOS and Linux always have a sandbox (`vendor/codex/sandboxing/src/manager.rs:62-76`), so the mode is correct there.

Two routes are closed: `ConfigOverrides` (`vendor/codex/core/src/config/mod.rs:2502-2530`) has no windows-sandbox field, and the v2 `thread/settings/update` params (`app-server-protocol/src/protocol/v2/thread.rs:218-271`) cannot carry it per thread. The TOML override is the only non-fork route. The key's spelling is `windows.sandbox = "unelevated" | "elevated"` (`vendor/codex/config/src/types.rs:157-171`), mapped at `core/src/config/mod.rs:3321-3325`.

`RestrictedToken` needs no elevation and no setup step: `app-server/src/request_processors/windows_sandbox_processor.rs:142-158` reports it `Ready` unconditionally; only `Elevated` needs `run_elevated_setup`.

### Changes

**Per D1, this is the whole fix:**

1. **Fork consistency patch** — `vendor/codex/core/src/apply_patch.rs:34`: pass `executor_windows_sandbox_level(turn_context.windows_sandbox_level, &action.cwd)` (already `pub(crate)` in `tools/sandboxing.rs`) instead of the raw level, so patch safety sees the same sandbox the write path already uses. Verified to return `AutoApprove` for the #294 repro under `OnRequest` with the level at `Disabled`, because a Windows-shaped cwd resolves to `RestrictedToken` through that helper. No config key needed.

**Moved out to the held sandbox ticket (D2), not part of #294:** writing `("windows.sandbox", "unelevated")` in `EngineSettings::cli_overrides()`. That is a posture change, not a bug fix, and its blast radius is described under "Risk pass" below.
2. **Do not** change the mode descriptions in `modes.rs`, and **do not** reach for `AskForApproval::Granular` (`vendor/codex/protocol/src/protocol.rs:941-956`): its `false` means auto-reject, not allow, and it is `#[experimental]` in the v2 wire copy.
3. Add one `tracing::info!` at approval resolution in `crates/atlas-native-agent/src/engine/approvals.rs` (decision, tool kind, tool id). The issue's unconfirmed "Allow → *Editing files* never completes" report is on this path and is currently undiagnosable from the log.

### Risk pass — belongs to the held sandbox ticket (D2), not to #294

Change 2 alone closes #294, and its blast radius is one branch. **Change 1 is the wide one and must be judged on its own**: it moves Windows from "read-only default, prompt on nearly every managed-filesystem command" to "workspace-write default, most commands run silently under a restricted token". Sites that flip: `vendor/codex/core/src/exec_policy.rs:751-780` (stops forcing `Prompt` and drops the known-safe short-circuit), `vendor/codex/core/src/tools/orchestrator.rs:244,429` (shell gains the restricted token), `vendor/codex/config/src/config_toml.rs:745-767` and `core/src/config/permissions.rs:51-57` (config-derived profiles stop being downgraded to read-only). What the unelevated token actually buys is **write containment only**: reads are not restricted (`vendor/codex/sandboxing/src/windows.rs:109-130` refuses to run rather than pretend), and "no network" is advisory env vars a program can ignore (`windows-sandbox-rs/src/env.rs:126-160`). New failure class to watch: Store/WindowsApps executables such as the `python.exe` execution alias, which have dedicated spawn-failure telemetry (`vendor/codex/core/src/exec.rs:544-595`). CI coverage is a manual script (`vendor/codex/windows-sandbox-rs/sandbox_smoketests.py`), not a test suite. Run `vendor/codex/core/tests/suite/windows_sandbox.rs`, then the manual matrix:

| Mode | `apply_patch` inside cwd | shell `git status` | shell `npm test` |
|---|---|---|---|
| Ask | prompts | prompts | prompts |
| Accept edits | **no prompt** | prompts | prompts |
| Bypass | no prompt | no prompt | no prompt |
| Plan | refused, nothing written | refused | refused |

### Tests

- `config.rs`: `on_windows_the_sandbox_key_is_written_so_accept_edits_can_auto_approve` (`cfg(windows)`), with a non-Windows twin asserting the key is absent.
- `vendor/codex/core/src/safety_tests.rs`: `Disabled` level + Windows cwd through the executor helper resolves to `AutoApprove`.
- `crates/atlas-native-agent/tests/engine_turn.rs`: `accept_edits_does_not_raise_an_approval_for_an_edit_inside_cwd`. No existing test asserts whether an edit prompts (the modes tests at `:587-687` cover acceptance and switching only), which is why this slipped.

---

## Step 2 · #289 — an interrupted turn comes back marked, with its tool calls and Retry; the mode survives a restart

### Root cause

**Transcript.** A reopened native session paints from the engine's `thread/resume` response via `crates/atlas-native-agent/src/engine/replay.rs::replay_turns`. It replays only `ThreadItem::UserMessage` and `AgentMessage`; the `_ => {}` arm at l.62-65 drops `CommandExecution`, `FileChange`, `McpToolCall` and `Reasoning`; and `turn.status` / `turn.error` are never read. A hard kill leaves the last `TurnStarted` with no `TurnComplete` or `TurnAborted`, which the engine projects as `TurnStatus::InProgress` (`app-server-protocol/src/protocol/thread_history_projection.rs:24-33`). The partial `AgentMessage` is replayed as an ordinary finished message. That is the whole of the "truncated reply looks finished" symptom.

The "rollout is lossy" comment in `replay.rs:10-18` is true only for `ThreadHistoryMode::Legacy`, which Atlas uses today: the thread store default is Legacy (`vendor/codex/thread-store/src/store.rs:65-67`) and Atlas never sets `ThreadStartParams.history_mode` (`v2/thread.rs:115`). In Legacy, `ExecCommandEnd` is transient (`vendor/codex/rollout/src/policy.rs:126`), so shell calls are never in the rollout, while `PatchApplyEnd` and `McpToolCallEnd` are (l.115-117). In `Paginated`, every `ItemCompleted(TurnItem)` is persisted (l.90-98); `ItemStarted` is not, so a call mid-flight at the kill is simply absent and every replayed item already has a terminal status.

**Mode.** `connection.rs:1252-1257` (`resume_session`) unconditionally applies the catalogue default:

```rust
let mode = self.default_mode.clone()
    .unwrap_or_else(|| acp::SessionModeId::new(modes::DEFAULT_MODE_ID));   // "default" = Ask
self.apply_mode(&engine_session_id, &mode).await?;
```

The frontend already persists the user's explicit pick in localStorage (`src/features/chat/lib/last-mode-pref.ts`, written on change, never on quit — so the hard kill is not the cause) and seeds it through `applyPersistedModePref` on resume. Then `session-sidebar.tsx:563` calls `setAcpModes(tab, snapshot.current_mode, …)`, which overwrites the seeded pref with the engine's forced `"default"`; `open-agent-session.ts:151-170` never re-applies it to the agent at all, so there the picker shows Bypass while the engine runs Ask. Neither resume path calls `agents.setMode`. The `session/new` path does the right thing at `chat-panel.tsx:429-453`; the resume paths have no equivalent.

Rejected alternatives for the mode: the engine's `session_modes` is an in-memory map (`connection.rs:348`); `atlas-thread-metadata` has no mode column; the engine restores `approval_policy` from the rollout on resume but `v2::Thread` carries no policy back, so Atlas could not keep the picker honest from that.

### Changes — Rust

1. **Paginated history for new native threads** — `connection.rs:1039-1048` (`ThreadStartParams`): `history_mode: Some(ThreadHistoryMode::Paginated)`. Gate: first confirm on a real thread that `thread/resume` of a Paginated thread returns `CommandExecution` items, and that `thread/fork` and `thread/rollback` (used by Retry) still work, including rollback of a turn whose `TurnStarted` has no terminal record. Existing Legacy threads keep replaying text + file changes + MCP calls.
2. **`crates/atlas-acp-thread/src/thread.rs`** (beside `push_assistant_content_block`, l.1317): add `pub fn push_assistant_notice(&mut self, text)` that always opens a new `AssistantMessage` entry through `push_entry`. Required because `push_assistant_content_block_with_message_id` (l.1330-1376) merges any chunk into the last assistant entry and `project::run_spans` (`crates/atlas-agent-delta/src/project.rs:44-58`) groups by `is_thought`, so a plain chunk would glue the marker onto the truncated sentence.
3. **`crates/atlas-native-agent/src/engine/sink.rs:280`**: `fn tool_call_of` → `pub(crate)`. It already maps `CommandExecution` (exit code → `Failed`), `FileChange` (locations + `patch`) and `McpToolCall` to `acp::ToolCall`; nothing to extract.
4. **`replay.rs`** — rewrite `replay_turns`:
   - `Reasoning` → `SessionUpdate::AgentThoughtChunk` (summary joined, else content; mirror `sink.rs:522-537`).
   - any item with `tool_call_of(item) == Some(call)` → `thread.upsert_tool_call(call)`; if the call is still `InProgress` and the turn is not `Completed`, force `Failed` first.
   - after the item loop, on `turn.status`: `InProgress` → `push_assistant_notice(INTERRUPTED_NOTICE)`; `Failed` → `push_assistant_notice("Error: {turn.error.message}")` (mirrors `chat-store.ts:2087`); `Interrupted` (a user stop) → no marker, matching the live path where `cancelled` is silent (`chat-store.ts:1943-1946`); `Completed` → nothing. Export the notice strings as `pub const`.
   - Do **not** call `set_error()` or `end_turn()` here. The thread must snapshot as `Idle` (`src-tauri/src/commands/agent_host.rs:1194-1200`) so `sessionCanRetry` (`src/features/chat/lib/retry-gate.ts:45-58`) holds and the existing Retry on the last user row (`transcript.tsx:847` → `retry-turn.ts:71`, rewind then resend) works unchanged.
   - Replace the module doc with the Paginated/Legacy truth.
5. `crates/atlas-agent-delta`: no projector change. `snapshot_messages` (`src/project.rs:437-498`) already projects `AssistantMessage` and `ToolCall` entries.

**Why a notice entry rather than a new delta or a synthetic `turn_finished`.** Restore paints only from `agents.snapshot()` → `snapshot_messages`, and `replaceMessages` (`chat-store.ts:1398`) wipes anything a transient delta pushed first. A synthetic `end_turn` in replay leaves no entry. A new `SessionDelta` would need per-turn outcome state on `AcpThread` and a change to the frozen `atlas-agent-wire` contract (`crates/atlas-agent-delta/src/lib.rs:7`) for no gain. The live path already uses exactly this shape: `stopReasonNotice` (`chat-store.ts:740-755`) pushes a `makeAssistantTextMessage` for `max_tokens`, and its comment — "reads like a finished one" — is the bug being reported.

### Changes — frontend

6. New `src/features/chat/lib/resume-mode.ts`: `applyModeOnResume(tabId, key, snapshot)` — the same resolution as `chat-panel.tsx:429-453` (explicit pref, validated against `snapshot.available_modes`), then `agents.setMode(key, effective)` when it differs from `snapshot.current_mode`, then seed the store (`setAcpModes` / `hydrateClaudePermissionMode`). Lift the validation into a pure `resolveEffectiveMode(requested, currentMode, availableModes)` and make chat-panel use it too.
7. `session-sidebar.tsx:562-568`: replace the bare `setAcpModes(...)` with `await applyModeOnResume(...)`, re-checking `isStale()` after the await. `open-agent-session.ts:161-165`: add the same call after `setAcpBinding`.
8. **Decision recorded:** restoring Bypass across a crash is acceptable. The pref is written only on explicit user picks (`last-mode-pref.ts:9-12`), this makes resume behave exactly like `session/new`, and native `set_mode` refuses only while a turn is busy, which cannot be the case right after a resume. Keep the engine's `apply_mode(default)` — it keeps picker and engine equal until the frontend re-applies.
9. Optional: render assistant rows equal to a notice constant (mirrored in `src/features/chat/lib/turn-notices.ts`) as a muted line in `transcript-rows.tsx`. Existing muted-text tokens only; no new visual pattern.

### Tests

- `replay.rs` (`crate::engine::test_support::detached_thread`): `command_and_file_change_items_replay_as_tool_call_entries`, `an_unfinished_turn_ends_with_an_interrupted_marker_as_its_own_entry`, `a_failed_turn_ends_with_its_error_message`, `a_completed_turn_gets_no_marker`, `reasoning_replays_as_a_thought_chunk`.
- `crates/atlas-acp-thread`: two `push_assistant_notice` calls make two entries; a notice after a chunk does not extend the chunk's entry.
- `crates/atlas-agent-delta/tests/projection.rs`: `a_notice_entry_snapshots_as_its_own_message`.
- `src/features/chat/lib/resume-mode.test.ts`: pref advertised → `setMode` called; pref not in `available_modes` → no call and store shows current; no explicit pref → no call; `setMode` rejects → falls back to `snapshot.current_mode`.
- `chat-store.deltas.test.ts`: `replaceMessages([user, assistant, notice])` keeps three messages with status `idle` and `sessionCanRetry` true.
- Manual: both repros from the issue (the 12 s kill and the five-file `apply_patch` kill). Expect the tool folds, the marker after the truncated text, Retry on the prompt, and Bypass still selected *and enforced* after relaunch.

### Risks

- Replay-drain race: `AgentHost::bind` (`agent_host.rs:1025`) drains replayed entries as live deltas that race `replaceMessages`. Tool upserts dedupe by id; a late `message_appended` could duplicate the marker. If seen, drop `message_appended` while `session.transcriptLoading` is true.
- Old crashed threads gain a marker retroactively on reopen. Correct, but visible.
- Replayed entries are stamped `Utc::now()` (`thread.rs:1239`); `Turn.started_at` is available if a `push_entry_at` is wanted later. Out of scope.

---

## Step 3 · #292 — `memory_forget` evicts the document index synchronously

### Root cause

- Shared entries are promoted into the document index by `read_shared_memory_docs` (`src-tauri/src/commands/agent_memory.rs:425-457`) with id `shared:{kind}:{id}`, `source: "shared"`, text `[{agent}] {content}`. The doc identity *is* the record entry id.
- `MemoryEngine::index_corpus` (`crates/atlas-memory/src/lib.rs:226-315`) deletes only by diffing the gathered corpus against the manifest. The pieces of a targeted delete all exist — `manifest.remove` (`manifest.rs:149`), `HnswStore::remove` (`store.rs:99`), `docstore.remove` (`docstore.rs:71`) — but only `reset_index` is public.
- The MCP tool (`memory_server/tools.rs:479-488`) calls `SharedMemoryStore::forget` (`shared_memory.rs:750-757`), which deletes the record and `announce()`s. The UI command `memory_forget_entry` (`shared_memory.rs:967-980`) additionally calls `enqueue_index`. The MCP path neither evicts nor nudges, so the stale doc lives in `docstore.json` until the turn-finished nudge (`agents.rs:472-474`) triggers a full corpus gather and MiniLM re-embed under the write lock. That is the "~2 minutes".
- `IndexSearch` (`tools.rs:81-90`) is search-only and `IndexDoc { title, source, text }` carries no id, so the search side cannot tell a stale shared doc from a live one.
- Adjacent gap found on the way: `memory_retrieve::retrieve_engine` uses `engine.try_read()` (`memory_retrieve.rs:89-95`) and returns nothing while a rebuild holds the write lock, so every reindex silently blanks the `documents` half of `memory_search`.

### Changes

1. `crates/atlas-memory/src/lib.rs`: `pub fn evict(&mut self, doc_id: &str) -> bool` — manifest + hnsw + docstore removal, then `persist()`. Reuse l.302-307.
2. `shared_memory.rs:348-380`: add `ids: Vec<i64>` to `MemoryChanged` so `announce` after a forget carries the entry id. `root` and `kinds` stay.
3. `memory_server/tools.rs`: `pub type IndexEvict = Arc<dyn Fn(String /*cwd*/, String /*doc_id*/) -> BoxFuture<'static, bool> + Send + Sync>`; `host.rs`: `Sources { index, bootstrap, evict: Option<IndexEvict> }`. Wire it in `agents.rs:680-705` to the indexer's engine handle with a bounded write-lock wait (≤ 2 s); on timeout fall back to `enqueue_index`.
4. `memory_forget` tool: after the record delete succeeds, build the doc id (move `shared_doc_id` to a shared helper under test) and call `evict`; also `enqueue_index` as the UI path does so a failed evict self-heals; only then return `{"forgotten": true}`.
5. Read-time filter in `memory_search`: add `id: Option<String>` to `IndexDoc` (from the docstore key); in `with_documents` (`tools.rs:294-309`) drop any doc whose id parses as `shared:{kind}:{id}` and whose entry is absent from the record. Never filter by text, and never filter a doc without a parseable shared id — this is the guard against the failure the thread feared, silently dropping live documents.
6. `memory_retrieve.rs:89-95`: replace `try_read()` with a timed `read()` (cap ~2 s) so a search during a rebuild waits briefly instead of returning nothing.

Belt and braces on purpose: change 4 closes the window; change 5 makes `{"forgotten": true}` a guarantee even if the evict path was skipped or timed out.

### Tests

- `crates/atlas-memory`: `evict_removes_manifest_vector_and_doc_text` (persist, reload, assert gone).
- `memory_server/tests.rs` (extend the `IndexSearch` mock pattern at `:372-395`): `forgetting_through_the_tool_evicts_the_document_before_returning`; `search_never_returns_a_shared_document_whose_entry_is_gone` (stale `shared:fact:45` omitted, live `shared:fact:44` kept, a non-shared doc untouched).
- `shared_memory.rs`: extend `forgetting_an_entry_removes_it_from_state_and_search` with the announced id.
- Manual: the QUOKKA repro — remember → forget → search in the same turn — returns no document hit.

---

## Step 4 · #286 — memory consultation cannot silently not-happen

### Root cause

The thread proved that the store, the server, retrieval and timing all work: whenever `memory_briefing` is called the answer is right, and a fact is searchable within 50 s of being written. The failure is that calling it is advisory and unobserved.

- The protocol is prose in `INSTRUCTIONS` (`memory_server/tools.rs:45-59`). On the native agent, MCP tools are deferred behind `tool_search` (`vendor/codex/core/src/tools/spec_plan.rs:223-245`), and the server's instructions become the *namespace description* of tools the model has to search for (`vendor/codex/codex-mcp/src/rmcp_client.rs:789-803`). `Feature::ToolSearchAlwaysDeferMcpTools` is `Stage::Removed` (`vendor/codex/features/src/lib.rs:1135-1146`); it cannot be turned off.
- Nothing records whether a session looked. `SessionClocks::last_look` (`memory_server/briefing.rs:53-75`) is `None` until the first `memory_briefing` or `memory_changes` call, but nothing reads it for that purpose. Analytics deliberately collapses every MCP tool name to `"mcp"` (`src-tauri/src/commands/tool_stats.rs:236-251`).

ADR-0010 chose pull over push to kill the prompt-prepend path: two truths, a 6 s + 8 s per-send budget, slash commands moved off byte 0, and Atlas's own text echoed through agent transcripts. Every change below leaves `agents_send` untouched and keeps the record reachable only through the tools, so this refines the ADR's consequences rather than reversing its decision. **Per D6 no ADR amendment is made.** Changes 1, 3 and 4 leave ADR-0010's decision intact: memory is still pulled, only the tools' visibility and the observability of a skipped pull change. An amendment would only be needed if change 2 is later un-held.

### Changes

1. **Un-defer the memory tools on the native agent** — `crates/atlas-native-agent/src/engine/mcp.rs:21-40`: add `config.insert(key("omit_tools_from"), json!(["deferred"]))` for `atlas_memory`. Gate: confirm at runtime that the per-thread dotted override lands in `McpServerConfig.omit_tools_from` (`vendor/codex/config/src/mcp_types.rs:207,345`); if it does not, use `features.code_mode.direct_only_tool_namespaces = ["mcp__atlas_memory"]` in `config.rs::cli_overrides()` instead (`vendor/codex/core/src/config/mod.rs:1058`). Result: the seven `memory_*` tools, with their descriptions, are in the model's initial tool list on every turn. This is the highest-leverage change and is native-agent-only; Claude Code already lists MCP tools directly.
2. **HELD (D5), do not build yet. Carry the protocol as developer instructions** — `connection.rs:1039-1048` and `:1209-1218`: populate `developer_instructions` (`v2/thread.rs:103`, `:387`) with a fixed text of ≤ 80 words, only when a memory server is offered for that session (sharing on, server running). Suggested: *"This project has Atlas shared memory. Before answering about project history, versions, conventions or past decisions, call `memory_briefing` once per session, or `memory_search`. Facts recorded there exist because they are not derivable from the code."* This is engine-side instruction text, not a prepend to the user's message, so the pollution loop and byte-0 concerns from ADR-0010 do not apply. **Held because it is nonetheless Atlas's words reaching the model without the agent asking, which is the shape ADR-0010 was protecting against. Post the recommendation on #286 first; it may prove unnecessary once change 1 lands and change 4 measures the result.**
3. **Put the "call first" line in the tool descriptions** — `tools.rs:132-219`: `memory_briefing`'s description opens with "Call this first in a session…". Change 1 makes descriptions the primary surface.
4. **Make non-consultation visible** — on the turn-finished path (`agents.rs:472-474`), when `SessionClocks::last_look(session)` is still `None`, surface one notice per session: "Memory not consulted this session". Reuse the notice-entry mechanism from Step 2 (or an existing `SessionDelta` if one fits); a small muted, dismissable line. Do **not** auto-call the tool from the app — that is the push path ADR-0010 deleted.

### Tests

- `engine/mcp.rs`: the projected override map contains `omit_tools_from = ["deferred"]` for the memory server.
- `connection.rs`: `developer_instructions` is set only when a memory offer exists.
- `memory_server/tests.rs`: `a_session_that_never_briefed_is_reported_as_unconsulted`; extend `the_instructions_tell_the_agent_to_pull_memory_first_and_when_to_write` to cover the briefing description.
- Manual: the Zig probe from the issue, five fresh threads on Claude Sonnet 4.6; expect `memory_briefing` or `memory_search` in every run's tool list and no "0.14.x" answer.

---

## Step 5 · #291 — time-to-first-token and zero cache reads on the Claude routes

### Root cause

- TTFT tracks the upstream call (Atlas overhead 0.3–1.4 s of a 1.7–12.4 s wait); there is no client-side latency to reclaim. `ttft_ms` is set on the first `OutputItemAdded` (`vendor/codex/core/src/client.rs:2240`).
- The request builder (`vendor/codex/codex-api/src/atlas_chat/request.rs:82-91`) is an 8-key allowlist; `prompt_cache_key` is asserted never sent (`request_tests.rs:462`); nested unknowns are a 400 (`docs/reference/atlas-ai-api.md:216-236`). No `cache_control` exists anywhere in `vendor/codex`, `crates`, `src` or the gateway spec.
- The gateway's Anthropic translation (`apps/ai/src/anthropic.ts`, server repo; reference copy at `~/Codes/atlas-server-ref/apps/ai/src/anthropic.ts`) builds `system` + alternating turns (l.224-292) and reads `cache_read_input_tokens` / `cache_creation_input_tokens` from the reply (l.464-465), but never emits `cache_control: {"type":"ephemeral"}` on the way out. Anthropic caches only with explicit breakpoints, so `cached_input_tokens` is always 0 on Claude. GLM reports reads because OpenAI-compatible providers cache prefixes implicitly. TTFT growing with transcript length is exactly an uncached, fully re-sent prompt.
- Compaction is configured by Atlas's catalogue at 90 % of `context_window` (`crates/atlas-native-agent/src/engine/catalog.rs:174-177`, pinned at `catalog_cache.rs:957-960`). Reasoning has no wire on this path (`request.rs:423-427`), so the effort setting cannot inflate TTFT.
- The whole pre-first-token window renders as "Thinking" (`src/features/chat/components/loading-state.tsx:93`, `transcript.tsx:189`).

### Changes

1. **Gateway ticket (server repo, `apps/ai/src/anthropic.ts`)** — file with the exact ask: add `cache_control: {type: "ephemeral"}` on (a) the last `system` text block, (b) the last `tools[]` entry, (c) the last two user-turn content blocks — the standard Anthropic breakpoint layout. Keep the existing usage mapping (`prompt_tokens = input + cache_read + cache_write`, `cached_tokens = cache_read`, `atlas-ai-api.md:358-365`). Note that the documented cache-write under-charge (`atlas-ai-api.md:369-375`) becomes live once caching is on. Owner: whoever has gateway access; this repo cannot fix it.
2. **Client prefix-stability audit (this repo)** — so the prefix is cacheable once the gateway caches: verify nothing per-turn varies the system text or the tool list order (`request.rs` message assembly; `spec_plan.rs` tool ordering; the memory server's `tools_list` is cached for 1 h so it is stable). Fix any per-turn timestamp or nondeterministic ordering found. One test: two consecutive requests in one thread share an identical byte prefix up to the new user message.
3. **Honest waiting state** — `loading-state.tsx` / `transcript.tsx`: before the first token show "Waiting for model…" (elapsed seconds after 3 s) instead of "Thinking", and switch to "Thinking" only when a reasoning or first item arrives. Existing shimmer; no new visual pattern.
4. **Keep the harness** — after the gateway change, re-run the issue's 39-turn sample and expect `cached_input_tokens > 0` from turn 2 onward and a flatter TTFT curve across turns.

### Tests

- Frontend: loading-state test for the two labels and the timer threshold.
- Rust: the prefix-stability test from change 2.

---

## Verification

| Step | Automated | Manual |
|---|---|---|
| 1 | `cargo test -p atlas-native-agent`; `vendor/codex/core/tests/suite/windows_sandbox.rs` on Windows | the mode matrix in Step 1, on Windows |
| 2 | `cargo test -p atlas-native-agent -p atlas-acp-thread -p atlas-agent-delta`; `bun run test` | both crash repros from #289 |
| 3 | `cargo test -p atlas-memory`; the `memory_server` tests in `src-tauri` | the QUOKKA repro from #292 |
| 4 | the above plus `bun run test` | the Zig probe, five runs on Sonnet 4.6 |
| 5 | `bun run test`; the prefix-stability test | TTFT re-measure after the gateway change |

After every step: `bun run typecheck && bun run lint`, then `bun run clean:rust`, then `graphify update .`.

## Open items this plan does not close

- **Approving a patch by hand writes it unsandboxed (D7, file upstream).** An auto-approved patch is written by a sandboxed helper process (`vendor/codex/core/src/tools/runtimes/apply_patch.rs:86-107` → `exec-server/src/sandboxed_file_system.rs:123-140`). A patch the user approves escalates, which clears `sandbox_requested` (`vendor/codex/core/src/tools/orchestrator.rs:423-447`), and the write becomes a direct in-process `tokio::fs::write` at full privilege (`exec-server/src/local_file_system.rs:562-571`). Clicking Allow is therefore the *less* contained path, on every platform including macOS. None of the five issues report it. Fixing it means deciding whether Allow should mean "run it contained" or "run it unrestricted", which is its own design question.

- #294's unconfirmed "Allow → *Editing files* never completes" report. Step 1 change 4 makes it diagnosable; nothing more until it recurs.
- #289's "revert an interrupted turn's file changes" follow-up. Retry re-runs the prompt; it does not undo the four files already written. That is a checkpoint/revert feature, not a restore bug.
- #286's observation that GLM 5.3 Flash wrote a near-duplicate derived fact on a read-only turn. The store's near-duplicate merge in `memory_remember` is the right home for that and is out of scope here.
