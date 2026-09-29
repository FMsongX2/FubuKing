# Q-0002: Accounts are CLI profiles; quota comes from official channels

Status: Accepted (2026-09-27), amended 2026-09-28 for the `fubuking` CLI and 2026-09-29 when the desktop app was removed (Q-0001). Accounts were first agent entries of that app; they are now the profile directories alone.

## Context

- Claude Code documents `CLAUDE_CONFIG_DIR`: each value gets its own settings, `.credentials.json` and macOS Keychain entry (code.claude.com/docs/en/authentication.md). The Codex CLI reads its home from `CODEX_HOME`.
- Claude Code documents subscription limits in exactly one machine-readable place: the statusLine JSON input, `rate_limits.five_hour` and `rate_limits.seven_day` with `used_percentage` and `resets_at` (code.claude.com/docs/en/statusline.md).
- Claude Code documents the limit-hit messages: "You've hit your session limit", "You've hit your weekly limit", model-specific and spend-limit variants (code.claude.com/docs/en/costs.md).
- The Codex app server answers `account/rateLimits/read` over JSON-RPC.
- Q-0001 forbids FubuKing from reading, copying or storing provider credentials.

## Decision

### Accounts

- An account is a profile: a provider (Claude or the OpenAI Codex CLI), a label, and a profile home directory `<config dir>/accounts/<base>-<label>/`, e.g. `claude-acp-work`. That directory listing is the registry. `fubuking login` creates the directory.
- The user's existing login is the implicit default profile. FubuKing never writes into `~/.claude` or `~/.codex`.
- Login runs the official CLI with the profile's variables: `fubuking login` starts `claude` (then `/login`) or `codex login`. FubuKing never sees the resulting tokens.
- Switching the account of a running conversation is a handoff to a session on another account. It is never an in-place environment change.

### Quota

- Codex profiles: FubuKing spawns `codex app-server` with the profile's `CODEX_HOME` and calls `account/rateLimits/read`. The CLI reads its own credentials.
- Claude profiles managed by FubuKing: the profile's own `settings.json` gets a statusLine command that stores the `rate_limits` object in `<profile>/fubuking-rate-limits.json`. The value is as fresh as the last interactive turn and is shown with its age.
- The implicit default Claude profile's settings are never edited. `fubuking claude` passes the same status line for that run with `--settings`, and the readings go to `<data dir>/claude-default/`. A status line the account's own settings set is kept: the script saves the JSON, then runs their command on it and prints what it prints. A project's status line is left to Claude, which runs it under its own trust rules (and not at all under `--restricted`); lifting it into a flag would bypass them. Nothing is added when the user's arguments carry `--settings`, `--setting-sources` or `--restricted`, since a second `--settings` would replace theirs.
- Every profile gets limit-hit detection. The CLI reads it from the session transcript: Claude Code records an assistant entry with `isApiErrorMessage: true` and `error: "rate_limit"`, Codex a `task_complete` event whose `error.codex_error_info` is `usage_limit_exceeded`. The last assistant entry or finished turn decides, so a limit followed by a successful turn is history, and it must be stamped after the run started: a resumed copy still ends with the previous account's limit.

### Limit handoff

- Nothing switches without asking.
- A change in either CLI's session format is reported, never guessed at. A CLI version outside the tested range is announced once per version; a session that ends on a limit in a form FubuKing does not read, or one saved where FubuKing does not look, is named when the run ends; `fubuking doctor` shows what FubuKing reads for the folder. None of these hands off.
- The CLI (`fubuking claude`, `fubuking codex`) hands off within one provider. When the CLI exits on a limit, it asks, copies the transcript to the same relative path under the next account's home (`projects/<encoded cwd>/<id>.jsonl`, `sessions/<yyyy>/<mm>/<dd>/rollout-…-<id>.jsonl`) and resumes that session there with `--resume <id>` or `resume <id>`, sending a message that says the session moved. The next account is the untried one with the most room, unknown room after known room, full accounts never. Neither CLI documents that lookup; it was checked with Claude Code 2.1.273 and Codex 0.147.0.
- The CLI attaches shared memory per run (`--mcp-config`, or `-c mcp_servers.fubuking.*` for Codex) and writes nothing into the CLI's own configuration.
- An interactive run is watched: when the transcript shows an account-level limit, the CLI is stopped (SIGINT, then SIGTERM, then SIGKILL, each to the CLI and its descendants in its process group, as Ctrl-C would reach them) and the terminal's saved `stty` state and modes are restored before the handoff question. On Windows the console stands in: a console Ctrl-C, then `taskkill /T /F` on the CLI's tree, and the console's saved input and output modes. A terminal that is not a Windows console is not watched, and the handoff waits for the CLI to exit. A CLI that npm installed on Windows is a `.cmd` file run by cmd.exe, which takes no line break in an argument and no command line over 8,191 characters, so its brief goes into a temporary file and the first message points to it. A Claude limit on one model ("switch to another model") is left open.
- A resumed session keeps the first run's options. Which words are options and how many values each takes is read from the CLI's own `--help` at handoff time; prompts and session selectors are dropped. `claude -p` and `codex exec` stay in that mode.
- When no account of the CLI has room, the other CLI starts with a brief of the session: the first and latest requests, the last replies, and a pointer to `git status` and `memory_briefing`. A copied session the CLI cannot find is started over the same way on that account, after asking: the same exit is what declining Claude's folder-trust question looks like (Codex's "Quit" exits 0 and simply ends the run). Before an interactive start on another account that has not trusted the folder, the CLI says which answer carries on. Claude keeps that answer per login in `.claude.json` (`projects[<folder>].hasTrustDialogAccepted`; an answer is taken to cover the folders under it, as Claude Code 2.1.283 appears to treat it). Codex keeps it in `[projects]` of the account's `config.toml`: the folder's own entry decides, else its repository's, which for a linked worktree is the main checkout and for a submodule does not exist (Codex 0.157.1). A folder Codex has marked untrusted gets its restricted screen, where "Open restricted" carries on. `claude -p` and `codex exec` do not ask. A copy already in the target account that went its own way is kept as a `.bak` before the new one replaces it.

## Consequences

- Claude quota for managed profiles lags until the profile has been used interactively; limit detection covers the gap.
- Quota readers never parse credential files, so an agent storage format change cannot break or leak anything in FubuKing.
