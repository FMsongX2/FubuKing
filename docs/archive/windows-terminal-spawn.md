> Historical — written before the rename in ADR-0011 (`vendor/codex` → `vendor/atlas-engine`, `codex-*` crates → `atlas-engine-*`, `"cersei"` → `"atlas-agent"`); paths and names below are as they were.

# Windows: a terminal window per Atlas-agent tool call

*Investigation report — 2026-09-14, branch `windows-first-run`, Windows 11 Pro 26200, Windows Terminal 1.24 as the default terminal host.*

## 1. The claim and the verdict

> "ACP agents (Claude Code, Codex) are fine with tool calls — no terminal or PowerShell window. With the Atlas agent, every tool call puts a terminal window on screen."

**Reproduced 2 of 2 valid runs for the Atlas agent, 0 of 1 for Claude Code (ACP). The claim is accurate; the user's attribution of *which* process opens the window is not quite: the window belongs to the engine's PowerShell command-safety parser, which the Atlas agent starts on its first shell tool call.** The tool command itself runs through a second ungated spawn path (`spawn_child_async`) that would open a window in exactly the same way; both are now gated.

Fix landed in this branch: `CREATE_NO_WINDOW` on every Windows-reachable child spawn in the engine (16 files), a leak-detector test that proves the mechanism, a source audit test that fails when a new ungated spawn appears, and an end-to-end probe script that records the screen and attributes every new console window to its owning process.

## 2. Method

Nothing here relies on watching the screen by eye. Every run was driven and recorded by a script:

* `scripts/windows/terminal-spawn-probe.ps1` (the committed successor of the scratch `record-session.ps1`) launches a **release** `atlas.exe` with a project, clicks the composer, types a prompt that makes the selected agent run a shell command, and for ~100 s captures
  * `frames/NNNNN_<t>s.png` — full-screen frames at 5 fps, DPI-aware,
  * `windows.log` — every *new visible* top-level window of class `ConsoleWindowClass` (legacy conhost) or `CASCADIA_HOSTING_WINDOW_CLASS` (Windows Terminal), with owning pid and title, found by `EnumWindows` + `IsWindowVisible`,
  * `processes.log` — every new process with pid, **ppid** and full command line,
  * `summary.json` — totals; exit code 2 when a window appeared, 0 otherwise.
* Agent selection: Alt+/ cycles the composer's agent; the Atlas agent is plugin `cersei` in Bypass mode (`sandbox_policy=DangerFullAccess`, `approval_policy=Never`).
* Prompt: *"Use your shell tool to run git status --short in this project and reply with the raw output only."*
* The engine log (`%LOCALAPPDATA%\dev.atlas.ide\logs\atlas.<date>.log`) confirms each turn was dispatched and the tool call executed.

**Why release only.** A debug `atlas.exe` is a console-subsystem binary (`main.rs`: `cfg_attr(not(debug_assertions), windows_subsystem = "windows")`). Its children inherit its console, so the leak cannot be seen in a dev build. This also invalidates the "count `conhost.exe`" metric used earlier: a `CREATE_NO_WINDOW` child still gets a *hidden* conhost, so the only meaningful measurement is *visible* console windows.

## 3. Evidence

| Run | Agent | New visible console windows | Window owner | Process that got the console |
|---|---|---|---|---|
| `rec-atlas-agent` | Atlas | — (void: composer click missed, no turn dispatched) | | |
| `rec-atlas-agent-2` | Atlas, Bypass | **2** at 32.92 s (`Terminal`) and 33.86 s (`…\powershell.exe`), pid 18176 = `WindowsTerminal.exe` | Windows Terminal | `powershell.exe` pid 8220, **ppid = atlas.exe**, started 30.43 s |
| `rec-claude-acp` | Claude Code (ACP) | **0** | | `powershell.exe` pid 22320, **ppid = claude.exe** (17916), started 31.37 s |
| `rec-atlas-agent-3` | Atlas, Bypass | **2** at 34.94 s and 35.31 s, pid 18176 | Windows Terminal | `powershell.exe` pid 14120, **ppid = atlas.exe**, started 31.88 s |

Frames: `rec-atlas-agent-3/frames/` around 35 s show the Terminal window over the Atlas window; `rec-claude-acp/frames/` shows none for the whole run. Recordings live in `%TEMP%\atlas-win-run\` (not committed; ~250 PNGs each).

Decoding the `-EncodedCommand` payload of the two Atlas-run `powershell.exe` processes gives:

```
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
# Long-lived PowerShell AST parser used by the Rust command-safety layer on Windows.
```

That is `vendor/codex/shell-command/src/command_safety/powershell_parser.ps1`. In both runs the `OpenConsole.exe -Embedding` host (Windows Terminal's console server) and the parser's `conhost.exe` appear within the same 200 ms sample as the window.

Engine log for run 3 (UTC):

```
05:23:56.655  feedback_tags: model="claude-sonnet-4-6" approval_policy=Never sandbox_policy=DangerFullAccess features=[ShellTool, …]
05:24:04.542  ToolCall: shell_command {"command": "Get-ChildItem -Recurse -Force | …"}
05:24:09.368  tool call completed
```

So the turn ran a shell tool call; the parser was spawned to classify it; the parser's console became a Terminal window. The tool command itself (`powershell.exe -Command …`, ~300 ms) fell between the recorder's 1 s process census — which is why only the long-lived parser shows in `processes.log`. The committed probe now censuses every frame.

## 4. Mechanism

1. Atlas is a GUI-subsystem process: it has **no console**.
2. On Windows, when a console-less parent creates a console-subsystem child (`powershell.exe`, `cmd.exe`, `git.exe`, `rg.exe`, `node.exe` …) without `CREATE_NO_WINDOW` (0x08000000) or `DETACHED_PROCESS`, the kernel allocates a **new console** for the child, even if all three stdio handles are pipes.
3. With Windows 11's default "Let Windows decide" terminal setting (`HKCU\Console\%%Startup`, `DelegationConsole`/`DelegationTerminal` empty), a new console is hosted by **Windows Terminal** (`OpenConsole.exe -Embedding`, window class `CASCADIA_HOSTING_WINDOW_CLASS`). It is created **visible**. Legacy conhost would have shown a `ConsoleWindowClass` window instead — same leak, different chrome.
4. A child that *does* carry `CREATE_NO_WINDOW` gets a hidden conhost (still a process — hence the earlier confusion) and no window.
5. **Grandchildren inherit.** `claude.exe` is spawned by Atlas *with* the flag (`atlas-process` helper), so when it runs `powershell.exe` for its own shell tool, the child attaches to claude's hidden console. That is the whole reason ACP agents look clean: they are one hop further from the GUI process.

`crates/atlas-process/tests/console_window.rs` demonstrates this from a test process that calls `FreeConsole()` to become console-less: a plain `cmd.exe /c ping …` with piped stdio opens a visible console window (`#[ignore]`d demo, passes with `--ignored`); the same spawn through `NoWindow::no_window()` opens none (the regression test, runs by default).

## 5. Exact trigger — the code path

Atlas agent, Bypass mode, shell tool call, Windows:

```
core/src/tools/handlers/shell            (tool call)
 └─ core/src/exec_policy.rs::render_decision_for_unmatched_command
     └─ is_safe_powershell_words → shell-command/command_safety/windows_safe_commands.rs
         └─ parse_with_powershell_ast
             └─ powershell_parser.rs::PowershellParserProcess::spawn      ← (A) std Command, piped, NO FLAG
 └─ core/src/exec.rs::execute_exec_request
     ├─ sandbox == WindowsRestrictedToken → exec_windows_sandbox            (not used in Bypass)
     └─ else exec()
         └─ core/src/spawn.rs::spawn_child_async                            ← (B) tokio Command, piped, NO FLAG
```

(A) is the window the recordings caught (long-lived, one per PowerShell flavour, cached for the process lifetime — so *the first* tool call opens it and it stays open until Atlas exits or the parser is recycled; the user perceives "a terminal window per tool call" because Terminal raises the window on every new tab/child).
(B) is the executor for every non-sandboxed tool command; each call is a fresh `powershell.exe` and would open its own window/tab.

The unified-exec / PTY path (`codex_sandboxing::spawn_process` → `codex-utils-pty` `win::ConPtySystem`, `CreateProcessW` with a pseudoconsole) was already correct, as was the pipe fallback (`pipe.rs`, patched in `00717ea9`) and every git spawn (`codex-git-utils`, job-object spawn with `CREATE_NO_WINDOW`).

Two further reachable sites fed the same symptom on first use: `shell_snapshot.rs:291` (dormant — snapshotting is "not supported yet for PowerShell") and the `pwsh`/`powershell` probes in `shell-command/src/powershell.rs` (`cmd /C pwsh …` and `powershell -Command Write-Output ok`) which run when the engine resolves its shell.

## 6. The gate

Every fix is the same one-liner, applied with a `// Atlas:` comment so it survives vendor rebases as a visible seam:

```rust
#[cfg(windows)]
cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
```

`tokio::process::Command` has `creation_flags` inherently; `std::process::Command` needs `use std::os::windows::process::CommandExt as _`. Atlas-owned code uses `atlas_process::command()` / `async_command()` / the `NoWindow` trait instead of the literal.

Patched in this change:

| Site | Command type | Reached by |
|---|---|---|
| `vendor/codex/core/src/spawn.rs` `spawn_child_async` | tokio | every non-sandboxed shell tool call (**B**) |
| `vendor/codex/shell-command/src/command_safety/powershell_parser.rs` | std | command-safety classification (**A**) |
| `vendor/codex/shell-command/src/powershell.rs` (2 probes, via `quiet()`) | std | shell resolution at session start |
| `vendor/codex/core/src/shell_snapshot.rs` | tokio | shell snapshot (dormant on PowerShell) |
| `vendor/codex/hooks/src/engine/command_runner.rs` (explicit shell + default `cmd.exe /C`) | tokio | Codex hooks (feature enabled: `CodexHooks`) |
| `vendor/codex/hooks/src/registry.rs` `command_from_argv` | tokio | hooks |
| `vendor/codex/login/src/auth/external_bearer.rs` | tokio | provider auth command |
| `vendor/codex/code-mode/src/remote_session/connection.rs` | tokio | code-mode host |
| `vendor/codex/app-server/src/request_processors/feedback_doctor_report.rs` | tokio | feedback doctor |
| `vendor/codex/core-plugins/src/marketplace_upgrade/git.rs` (`no_console_window`) | std | marketplace upgrade |
| `vendor/codex/exec-server/src/client_transport.rs`, `fs_sandbox.rs`, `connection.rs` (taskkill) | tokio / std | exec-server transport |
| `vendor/codex/rollout/src/search.rs` (`rg`) | tokio | rollout search |
| `vendor/codex/windows-sandbox-rs/src/setup.rs` (refresh) | std | sandbox setup |
| `vendor/codex/terminal-detection/src/lib.rs` (tmux/zellij probes, via `quiet()`) | std | terminal detection (binaries absent on Windows; gated for completeness) |
| `src-tauri/src/commands/agents.rs` (`agents_run_auth_method`) | tokio → `atlas_process::async_command` | headless agent login |
| `src-tauri/src/commands/byok.rs`, `fs.rs` | std → `atlas_process::command` | unix-shaped probes; gated so the audit stays uniform |

Already gated before this change (for the record): `atlas-process` callers across `src-tauri` and `crates/`, `codex-git-utils` (job object), `codex-utils-pty` `pipe.rs` and `win/job.rs`, `rmcp-client` stdio MCP servers (`prepare_suspended_spawn`) and `http_headers.rs` (`spawn_contained`).

Known gap, deliberately left: `windows-sandbox-rs/src/bin/command_runner/win/cwd_junction.rs` spawns `cmd.exe` from inside the sandbox runner, itself a console process whose console the child inherits — no window.

## 7. Tests

| Test | Kind | What it proves |
|---|---|---|
| `crates/atlas-process/tests/console_window.rs::no_window_helper_keeps_a_console_child_windowless` | integration, Windows, runs by default | from a console-less process, `NoWindow` yields 0 visible console windows |
| `…::a_plain_command_from_a_windowless_parent_opens_a_console_window` | `#[ignore]` demo (opens a real window) | the unflagged spawn opens ≥ 1 window — the mechanism, not a guess |
| `crates/atlas-process/tests/spawn_audit.rs::every_windows_reachable_spawn_is_gated_or_a_known_gap` | source audit, all platforms | every `Command::new(` in `src-tauri/`, `crates/`, `vendor/codex/` that Windows can reach has a gate within 30 lines, or is in `KNOWN_GAPS`; fails on a new ungated spawn **and** on a stale `KNOWN_GAPS` entry |
| `scripts/windows/terminal-spawn-probe.ps1` | e2e, release exe, manual/CI-on-a-desktop | exit 2 if any console/terminal window appears during an agent shell tool call; leaves frames + attributed logs |

Run: `cargo test -p atlas-process --test console_window --test spawn_audit`; `cargo test -p atlas-process --test console_window -- --ignored` for the demo.

The audit is heuristic by design (it skips `#[cfg(unix)]`/macOS-only items, test modules and tooling; it recognises `StdCommand`/`AsyncCommand` aliases and the gating helpers). It is cheap, runs on every platform, and is the thing that stops the next `Command::new` from re-introducing the bug.

## 8. Action plan

**Done in this branch**
1. Gate (A) and (B) plus the 14 other reachable engine/app sites listed in §6.
2. Leak-detector and audit tests; probe script.
3. Release build + MSI rebuilt from the patched tree; run #5 of the probe on that build (see §9).

**Next (ordered by value)**
4. **Run the probe in CI on a Windows desktop runner** (needs an interactive session — a service session has no window station, so the leak can't reproduce there). Gate `bun run build:app:win` on exit 0 for both the Atlas agent and one ACP agent.
5. **Prefer the helper over the literal.** Move the raw `0x0800_0000` in vendor files to `codex_git_utils::no_console_window` (std) / a small `codex_utils_pty::no_window` (tokio) so the vendor diff is one import per file and rebases are mechanical. Keep the `// Atlas:` marker.
6. **Centralise in the seam.** The clean long-term fix is in `spawn_child_async`, `codex_sandboxing::spawn_process` and `StdioServerLauncher` only — the three chokepoints every tool path funnels through — and the audit `KNOWN_GAPS` becomes the list of leaf sites that don't matter. Upstreaming to openai/codex is plausible: the flag is harmless for the CLI (a console child inherits the CLI's console regardless of the flag), so it costs upstream nothing.
7. **Watch the `RunAsUser`/sandbox path.** When Windows sandboxing (`WindowsRestrictedToken`) is enabled, `exec_windows_sandbox` → `windows-sandbox-rs` `command_runner` takes over; its own `CreateProcessAsUserW` call site should be audited the same way (the `cwd_junction.rs` gap is inside it). Not exercised by Bypass mode, so not covered by the recordings.
8. **ConPTY caveat.** A pseudoconsole child has no window of its own; if a future path switches from pipes to `win::ConPtySystem` it is safe, but children *of* that child that call `AllocConsole` still are. Nothing in the tree does today.
9. **Default-terminal sensitivity.** With "Windows Console Host" as the default terminal the leak shows as a black conhost window instead of a Terminal tab; with `DETACHED_PROCESS` a child would have *no* console at all and `powershell.exe` would fail to start. `CREATE_NO_WINDOW` is the only correct flag; never "fix" this by changing the user's default terminal.
10. **Remove the audit's 30-line window** once the seam (item 6) lands: at that point the audit can require the helper by name and drop the proximity heuristic.

## 9. Verification on the patched build

Build: `bun run build:app:win` from the tree with the `spawn.rs`, `shell_snapshot.rs` and `shell-command` gates (2026-09-14 12:10). Probe:

```
scripts\windows\terminal-spawn-probe.ps1 -Exe F:\atlas-target\x86_64-pc-windows-msvc\release\atlas.exe `
    -Project %TEMP%\atlas-win-run\sample-project -OutDir rec-atlas-agent-5 -Seconds 100
probe: frames=247 newWindows=0 newProcesses=40 -> …\rec-atlas-agent-5   (exit 0)
```

With the per-frame census both spawns on the tool-call path were caught this time, and neither produced a window:

| t | process | ppid | path |
|---|---|---|---|
| 26.69 s | `powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand …` (the AST parser) | 20068 = `atlas.exe` | (A) `powershell_parser.rs` |
| 30.47 s | `powershell.exe -Command "try { [Console]::OutputEncoding=… } …git status --short"` (the tool command) | 20068 = `atlas.exe` | (B) `spawn_child_async` |

Engine log: `06:11:21 ToolCall: shell_command {"command": "git status --short"}` → `06:11:25 tool call completed`, `sandbox_policy=DangerFullAccess`. Twelve `conhost.exe` were created during the run — all hidden, the expected shape of `CREATE_NO_WINDOW` children — and no window of either console class appeared in 247 frames. `rec-atlas-agent-5/frames/00061_31.18s.png` is the same moment as run 3's frame 85: Atlas, no Terminal.

Same prompt, same agent, same machine, same default terminal: before the gate 2/2 runs opened a window; after it 0/1. The remaining gates (§6, everything outside `core` and `shell-command`) are not on this path and were compile-checked and audit-checked; they ship in the MSI built after this probe.

Run 6 (12:46 build, every gate in §6, `Atlas_0.3.2_x64_en-US.msi`): `frames=252 newWindows=0 newProcesses=23`, exit 0 — the parser at 25.12 s (ppid = atlas.exe) and the tool command at 28.73 s, both windowless. Score after the gate: 0 windows in 2 of 2 runs.
