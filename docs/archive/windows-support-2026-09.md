> Historical — written before the rename in ADR-0011 (`vendor/codex` → `vendor/atlas-engine`, `codex-*` crates → `atlas-engine-*`, `"cersei"` → `"atlas-agent"`); paths and names below are as they were.

# Windows support — every change from the September 2026 session, with its macOS exposure

*Branch `windows-first-run` → PR into `0.3.2`. Base: `81e6384c` (main after PR #257). Eleven commits, 106 files, 2 245 insertions / 254 deletions.*

This is the hand-off note for whoever verifies the branch on macOS. Atlas was a macOS-only build until this session; every change below was made to get it running, packaged and quiet on Windows 11, and none has been run on a Mac. Each entry says what changed, why, and — the part that matters for the reviewer — **what a Mac could see differently**, rated:

* **none** — code is `cfg(windows)` / `isWindows`-gated or a no-op off Windows; a Mac executes exactly what it did before.
* **compile** — new crate/dep/config that the Mac build must still compile and bundle, no runtime path changes.
* **behaviour** — a code path both platforms run now does something different. Verify on a Mac; if it breaks, the fix should be a common-ground change, not a platform fork.

Where a "behaviour" item breaks on macOS, the intent is recorded so the Mac-side agent can pick a design that keeps both platforms working rather than reverting the Windows fix.

---

## 1 · `0da2328f` — first Windows run: startup, chrome, terminal, paths

| Change | Files | macOS exposure |
|---|---|---|
| PATH enrichment (`$SHELL -lic` probe, nvm walk, POSIX extras) is now `#[cfg(unix)]`. On Windows it split PATH on `:` and corrupted it at boot. | `crates/atlas-agent-servers/src/host_env.rs` | **none** — the unix body is unchanged, only wrapped. |
| `AWS_LC_SYS_PREBUILT_NASM=1` in the cargo env so `aws-lc-sys` links its prebuilt NASM objects on MSVC. | `.cargo/config.toml` | **compile** — aws-lc-sys documents the var as x86_64-Windows-only; on a Mac it is read and ignored. If a Mac build ever fails in aws-lc-sys after this, this line is the first suspect. |
| Frameless window on Windows via `tauri.windows.conf.json` (platform overlay file, Windows-only by Tauri's naming). | `src-tauri/tauri.windows.conf.json` | **none** — Tauri only merges the overlay on Windows. |
| Titlebar: minimize / maximize / close buttons and double-click-to-maximize on Windows; macOS traffic-light insets and the app menu are now gated on `isMac` / `cfg(target_os = "macos")`. | `src/components/titlebar.tsx`, `src-tauri/src/lib.rs` (`mod menu` + `.menu(menu::build)` under `cfg(target_os = "macos")`), `src-tauri/capabilities/default.json` (`allow-minimize`, `allow-toggle-maximize`, `allow-close`) | **behaviour (verify)** — on a Mac the rendered output should be identical, but it now depends on `isMac` being true. `isMac` is a UA sniff (`navigator.userAgent.includes("Macintosh")`, `src/lib/platform.ts`) because Atlas ships no `plugin-os`. WKWebView reports "Macintosh"; if a future WebKit changes the UA the traffic-light gap disappears and the Windows buttons appear. **Common ground if it breaks:** add `@tauri-apps/plugin-os` and derive `isMac` from `platform()` once at boot; keep the same exported booleans. |
| `src/lib/platform.ts` — `isMac`, `isWindows`. | new | **behaviour** — see above; every gate in this branch keys off these two. |
| Shortcut labels: `displayKeys` / `displayLabel` render `Ctrl`/`Alt`/`Shift` joined by `+` on Windows. | `src/features/keybindings/lib/combo.ts` | **none** — the mac branch (`⌃ ⌥ ⇧ ⌘`, no joiner) is untouched. |
| Terminal: default shell is `powershell.exe` on Windows; `-l` is only passed off Windows; Enter is written as `\r` on Windows (`\n` elsewhere). | `crates/atlas-terminal/src/lib.rs`, `src/features/terminal/lib/terminal-session.ts` (`ENTER` constant replaces four literal `"\n"`s) | **none** — mac still gets `$SHELL -l` and `\n`. Worth one smoke test that Enter, `clear` and the password field still work in the Mac terminal since the literals were replaced by a constant. |
| `basename()` treats `/` and `\` alike; project/workspace names come from it instead of `path.split("/").pop()`; saved workspaces whose `name === path` are re-derived on hydrate. | `src/lib/paths.ts`, `src/App.tsx`, `src/features/workspaces/stores/workspace-store.ts`, `src/features/explorer/components/file-tree.tsx` | **behaviour** — a macOS folder whose *name contains a backslash* (legal on APFS) now displays the part after the last `\`. Vanishingly rare; if it matters, make `basename` split on `\` only when `isWindows`. Nothing else changes: for `/`-only paths the result is identical. |
| `dunce::canonicalize` instead of `std::fs::canonicalize` in workspace ids, capture ids, CLI project path, fs, terminal (strips Windows `\\?\` prefixes). | `src-tauri/src/commands/{capture,cli,fs,gitdiff,memory_timeline,session_chat,skills,terminal,git,github,knowledge_export}.rs` | **none** — `dunce` is a thin wrapper that calls `std::fs::canonicalize` unchanged on non-Windows. Compile: one new dependency. |
| The bash `atlas` CLI helper is not installed on Windows (`cli_install_helper` returns an error; `App.tsx` skips the boot refresh when `isWindows`). | `src-tauri/src/commands/cli.rs`, `src/App.tsx` | **none** — mac path unchanged. |
| Global memory dir falls back to `%USERPROFILE%` when `HOME` is unset. | `crates/atlas-memory/src/global.rs` | **none** — `HOME` is always set on macOS. |

## 2 · `f737dfcc` — settle the UI around agent installs and removals

| Change | Files | macOS exposure |
|---|---|---|
| Install status: archive installs always clear "Installing…"; the progress subscription is taken *before* the connect starts (first message was lost); the 3-minute client deadline **re-arms while the install is still reporting progress** (`withDeadline(…, stillWorking)`). | `crates/atlas-agent-manager/src/manager.rs`, `crates/atlas-agent-store/src/servers.rs`, `src/features/chat/lib/with-deadline.ts`, `src/features/chat/stores/chat-store.ts` | **behaviour** — cross-platform by design: a slow first install on a Mac (a big npm tree on a slow link) no longer gets killed at 3 min and restarted; it can now take as long as it keeps reporting progress. If a Mac install stalls *while* emitting progress, it will hang instead of failing — the "still working" predicate is the thing to tighten (e.g. require progress text to *change*), not the re-arm. |
| Registry targets naming their binary bare (`amp-acp.exe`) are accepted by `relative_target_cmd` (resolved against the archive root). | `crates/atlas-agent-store/src/servers.rs` (+ tests) | **behaviour (low)** — the same relaxation applies to bare names in mac registry entries (`amp-acp`), which previously failed after download. Only widens what resolves; nothing that resolved before resolves differently. |
| Removed agents: `removed-agents.ts` watches the catalog shrink; tabs bound to a removed agent fall back to the native agent (untouched tabs) or get a disconnected banner with "Switch agent" (tabs with a conversation). Wired in `App.tsx` (`watchRemovedAgents()`). | `src/features/chat/lib/removed-agents.ts` (+ test), `src/features/chat/stores/chat-store.ts` (`pending-bind` test), `src/App.tsx` | **behaviour** — cross-platform UI logic; uninstalling an agent on a Mac now re-binds its tabs. Verify: uninstall an agent with an open tab on it → banner with Switch agent; untouched tab → native agent pill. |

## 3 · `1d8708d5` — in-app confirm before removing an agent

| Change | Files | macOS exposure |
|---|---|---|
| Marketplace "Remove" used `window.confirm`, which WebView2 answers `true` with no dialog. Replaced by a Radix dialog on the `ask()/settle()` pattern (same as the stop-agents prompt). Esc / overlay click keep the agent. | `src/features/agents/components/remove-agent-dialog.tsx`, `src/features/agents/lib/remove-agent-confirm.ts` (+ test), `src/features/marketplace/agents-marketplace/agents-marketplace.tsx`, `src/App.tsx` (`<RemoveAgentDialog />`) | **behaviour** — on a Mac the native confirm sheet is replaced by the in-app dialog. Intended (consistent look on both platforms). Verify it opens, cancels, and removes. **Rule for new code:** never use `window.confirm` / `alert` — WebView2 makes them silently succeed. |

## 4 · `77444aba` — removed-agent notice as a composer strip

| Change | Files | macOS exposure |
|---|---|---|
| The floating "no longer installed" banner becomes a strip under the composer, sharing construction with the no-grant bar via `composer-strip.ts`. | `src/features/chat/components/{removed-agent-bar,composer-strip,ai-grant-bar,chat-panel,message-input}.tsx/.ts` | **behaviour** — pure UI, cross-platform; the no-grant bar was refactored onto the shared helper, so check that bar still renders (BYOK without a grant). |

## 5 · `eb64bbfb` — Windows `.msi` package, `bun run build:app:win`

| Change | Files | macOS exposure |
|---|---|---|
| `build:app:win` script (same `with-posthog-env.mjs` wrapper as the mac scripts; target `x86_64-pc-windows-msvc`, bundle `msi`). | `package.json` | **none**. |
| Bundle target `msi`, publisher/homepage, silent WebView2 bootstrap. | `src-tauri/tauri.windows.conf.json` | **none** (Windows overlay). |
| **License resources moved.** `resources` in the shared `tauri.conf.json` pointed `../LICENSE` and `../vendor/codex/LICENSE` at two destinations that both had basename `LICENSE`; WiX refused (ICE30). They now live at `src-tauri/licenses/{Atlas-LICENSE,OpenAI-Codex-LICENSE,OpenAI-Codex-NOTICE}.txt` and the map is identity. | `src-tauri/tauri.conf.json`, `src-tauri/licenses/*` (new, copies) | **compile / bundle (verify)** — the DMG's `Resources/licenses/` should be byte-identical to before. The files are *copies*: if `LICENSE` or `vendor/codex/LICENSE` changes, the copies must be updated (a tiny script or a CI diff check is the common-ground fix; the mac bundler could also go back to reading the originals with a mac-only overlay, but two sources of truth is worse than two copies). |

## 6 · `eb6a5ea0`, `bbaf5f4f` — `atlas-process`: no console window for any child Atlas spawns

| Change | Files | macOS exposure |
|---|---|---|
| New leaf crate `crates/atlas-process`: `command()`, `async_command()` and a `NoWindow` trait for std/tokio commands that set `CREATE_NO_WINDOW` on Windows and are a **no-op elsewhere**. All 37 Atlas spawn sites (`src-tauri`, `atlas-git`, `atlas-checkpoint`, `atlas-terminal`, `atlas-agent-store`, `atlas-agent-servers`, `atlas-native-agent`) go through it. | `crates/atlas-process/**`, `Cargo.toml` (member + dev opt-level), every crate's `Cargo.toml` (dep), the spawn sites | **compile** — each crate gains a dependency on `atlas-process` (which depends on `tokio`). Runtime on a Mac: `atlas_process::command(x)` *is* `std::process::Command::new(x)`. The unix/macOS-only spawns (`$SHELL -lic` probes in `byok.rs`, `screencapture` in `fs.rs`, `lsof`) were later also routed through it for uniformity — same no-op. |

## 7 · `00717ea9` — no console window for the engine's (vendored Codex) child processes

| Change | Files | macOS exposure |
|---|---|---|
| `// Atlas:`-marked patches in `vendor/codex`: `utils/pty/win/job.rs` (`CREATE_SUSPENDED \| CREATE_NO_WINDOW`), `git-utils` (`no_console_window` helper, `CREATE_NO_WINDOW` const, applied in `apply.rs`, `operations.rs`, job-object fallback), `core-plugins` (startup sync git, marketplace git, loader/install, `npm pack`), `hooks` fallback + `taskkill`, `utils/pty/pipe.rs`. | vendor | **none** — every line is `#[cfg(windows)]` or inside a Windows-only module. **Maintenance cost, not a Mac risk:** the next `vendor/codex` sync must carry these hunks; grep for `// Atlas:` to find them (see §9 for the audit test that fails if one is lost). |

## 8 · `a36f8077` — `curatedPluginSync` setting gates the engine's plugin-catalogue `git fetch`

| Change | Files | macOS exposure |
|---|---|---|
| The engine cloned `github.com/openai/plugins` at every launch (shallow fetch + reset, HTTP-archive fallback) and was getting 429s. New `AppSettings.curatedPluginSync` (**default `false`**), Settings → Updates toggle, carried to the in-process engine as env `ATLAS_CURATED_PLUGIN_SYNC=1` at boot (`lib.rs`) and on every settings commit; vendored `start_curated_repo_sync` returns early when unset. | `src-tauri/src/state/atlas_config.rs`, `src-tauri/src/commands/atlas_config.rs`, `src-tauri/src/lib.rs`, `src/features/settings/lib/app-settings.ts`, `src/features/settings/components/settings-panel.tsx`, `docs/reference/configuration.md`, `vendor/codex/core-plugins/src/manager.rs` | **behaviour — the one real product change on macOS.** With the default, a Mac no longer refreshes Codex's curated plugin catalogue at launch. Anything in the Atlas UI that lists or installs *curated* plugins from that catalogue (as opposed to the Atlas agent marketplace, which is separate) will show a stale or empty catalogue until the user turns the setting on. If that surface matters on the Mac build, the common-ground options are: (a) default the setting to `true` on macOS only (`cfg!(target_os = "macos")` in `AtlasConfig::default`), (b) keep it off but sync lazily the first time the catalogue UI opens, or (c) keep it off everywhere and document it. The env-var plumbing works for all three. |

## 9 · `d7699ae0`, `c720d5b8` — no console window for the engine's tool-call spawns; audit, probe, report

| Change | Files | macOS exposure |
|---|---|---|
| `CREATE_NO_WINDOW` on the two spawns on the Atlas-agent tool-call path (`core/src/spawn.rs::spawn_child_async`, `shell-command/…/powershell_parser.rs`) and 14 more Windows-reachable sites (hooks, provider auth, code-mode host, doctor report, marketplace git, exec-server, rollout `rg`, sandbox setup, terminal-detection probes, and `agents.rs` / `byok.rs` / `fs.rs` via `atlas-process`). | vendor + `src-tauri/src/commands/{agents,byok,fs}.rs` | **none** at runtime (all `cfg(windows)` or no-op helper). Two vendor sites were restructured rather than one-lined — `rollout/src/search.rs` (the `rg` builder chain was split into `let mut command = …; command.arg(…)…;`) and `terminal-detection/src/lib.rs` (tmux/zellij probes bound to a variable) — the behaviour is identical but they are the two hunks worth reading on a rebase. |
| `crates/atlas-process/tests/console_window.rs` — Windows-only leak detector (`#![cfg(windows)]`). | new | **none** — compiles to nothing on a Mac. |
| **`crates/atlas-process/tests/spawn_audit.rs` — runs on every platform.** Walks `src-tauri/src`, `crates`, `vendor/codex` and fails if a `Command::new(` reachable on Windows has no gate within 30 lines (`.no_window()`, `no_console_window(`, `.creation_flags(`, `quiet(&mut`, `.spawn_contained(`, `.prepare_suspended_spawn(`, `run_git_command_with_timeout`) and isn't in `KNOWN_GAPS`. It skips test modules, `#[cfg(unix)]`/macOS-attributed items and files, and known tooling. | new | **behaviour for contributors** — a Mac developer who adds a plain `Command::new` will see this test fail in `cargo test -p atlas-process`. The fix is one of: use `atlas_process::command()` / `async_command()`; if the code is genuinely mac/unix-only, put `#[cfg(unix)]` or `#[cfg(target_os = "macos")]` on the *item* (the audit reads the attribute); or, for vendor code, add the `#[cfg(windows)] creation_flags` line. Do not add to `KNOWN_GAPS` without a reason comment. |
| `scripts/windows/terminal-spawn-probe.ps1` — e2e recorder for a release exe. | new | **none**. |
| `docs/research/windows-terminal-spawn.md` — the investigation report. | new | **none**. |

---

## What to run on the Mac before merging further

1. `cargo check --workspace` and `cargo test -p atlas-process` (the audit must pass; `console_window` is skipped on macOS).
2. `bun run typecheck:app typecheck:test` and the vitest suites touched: `paths.test.ts`, `remove-agent-confirm.test.ts`, `removed-agents.test.ts`, `with-deadline.test.ts`, `chat-store.pending-bind.test.ts`, `crates/atlas-agent-store/tests/store.rs`.
3. `bun run build:app` (mac) — confirm the DMG still carries `Resources/licenses/` (§5) and that the app menu (Cmd+W → Close Tab) still exists (§1, `cfg(target_os = "macos")`).
4. Launch: traffic-light inset present, no Windows buttons in the titlebar (§1); terminal Enter / `clear` / password prompt (§1); shortcut hints still show `⌘` glyphs (§1).
5. Agents: install one (progress text appears immediately and a slow install is not killed at 3 min — §2); remove one with an open tab (in-app dialog, then the composer strip and Switch agent — §§2–4).
6. Settings → Updates: `curatedPluginSync` toggle exists; decide the macOS default (§8) — this is the one item that needs a product decision, not just a test.

## Conventions this branch introduces (keep them)

* Spawn a process → `atlas_process::command` / `async_command` / `NoWindow`. Never a bare `Command::new` in Atlas code.
* Platform gates: Rust `cfg!(windows)` / `#[cfg(target_os = "macos")]`; TS `isMac` / `isWindows` from `@/lib/platform`.
* No `window.confirm` / `window.alert`.
* Paths for display go through `basename` / `shortPath`, never `split("/")`.
* Patches inside `vendor/codex` carry a `// Atlas:` comment.
