# Q-0002: Accounts are agent entries; quota comes from official channels

Status: Accepted (2026-09-27), amended 2026-09-28 for the `quotatlas` CLI

## Context

- Atlas runs one agent process per agent id and serves every session and project from it. The child environment is fixed at spawn; there is no per-session environment hook.
- The top environment layer is the `env` of an entry in `<app_data>/external-agents/installed.json`. It applies at the next spawn.
- Claude Code documents `CLAUDE_CONFIG_DIR`: each value gets its own settings, `.credentials.json` and macOS Keychain entry (code.claude.com/docs/en/authentication.md). The Codex CLI reads its home from `CODEX_HOME`.
- Claude Code documents subscription limits in exactly one machine-readable place: the statusLine JSON input, `rate_limits.five_hour` and `rate_limits.seven_day` with `used_percentage` and `resets_at` (code.claude.com/docs/en/statusline.md).
- Claude Code documents the limit-hit messages: "You've hit your session limit", "You've hit your weekly limit", model-specific and spend-limit variants (code.claude.com/docs/en/costs.md).
- The Codex app server answers `account/rateLimits/read` over JSON-RPC.
- Q-0001 forbids Quotatlas from reading, copying or storing provider credentials.

## Decision

### Accounts

- An account is a profile: a provider (Claude or the OpenAI Codex CLI), a label, and a profile home directory under `<app_config>/accounts/<id>/`, with `@` in the id made path-safe. That directory listing is the registry the `quotatlas` CLI reads, so both see the same accounts. `quotatlas login` creates the directory and leaves a `.quotatlas-adopt` marker; the app gives a marked profile an entry when it lists accounts or refreshes quota, one adoption at a time, and removes the marker. An account removed in the app keeps its profile home but has no marker, so it stays removed.
- Each profile is registered as its own installed agent entry, cloned from the base agent's distribution, with the profile home in its `env` (`CLAUDE_CONFIG_DIR` or `CODEX_HOME`). The entry's display name carries the label, e.g. "Claude Code · work".
- The user's existing login is the implicit default profile. Quotatlas never writes into `~/.claude` or `~/.codex`.
- Login runs the official CLI in a Quotatlas terminal with the profile environment (`claude` then `/login`, or `codex login`). Quotatlas never sees the resulting tokens.
- Switching the account of a running conversation is a handoff to a new session on another entry. It is never an in-place environment change.

### Quota

- Codex profiles: Quotatlas spawns `codex app-server` with the profile's `CODEX_HOME` and calls `account/rateLimits/read`. The CLI reads its own credentials.
- Claude profiles managed by Quotatlas: the profile's own `settings.json` gets a statusLine command that stores the `rate_limits` object in `<profile>/quotatlas-rate-limits.json`. The value is as fresh as the last interactive turn and is shown with its age.
- The implicit default Claude profile's settings are never edited. `quotatlas claude` passes the same status line for that run with `--settings`, and the readings go to `<app_data>/claude-default/`. A status line the account's own settings set is kept: the script saves the JSON, then runs their command on it and prints what it prints. A project's status line is left to Claude, which runs it under its own trust rules (and not at all under `--restricted`); lifting it into a flag would bypass them. Nothing is added when the user's arguments carry `--settings`, `--setting-sources` or `--restricted`, since a second `--settings` would replace theirs.
- Every profile gets limit-hit detection. The CLI reads it from the session transcript: Claude Code records an assistant entry with `isApiErrorMessage: true` and `error: "rate_limit"`, Codex a `task_complete` event whose `error.codex_error_info` is `usage_limit_exceeded`. The last assistant entry or finished turn decides, so a limit followed by a successful turn is history, and it must be stamped after the run started: a resumed copy still ends with the previous account's limit.
- Quota state reuses the wire type `RateLimitWindow` so the existing rate-limit UI can render it.

### Limit handoff

- When the active profile is exhausted, the chat offers to continue on another profile or agent. Continuing opens a new session on that entry with the shared-memory handoff Atlas already builds for agent switches.
- Nothing switches automatically unless the user enables it per project.
- The CLI (`quotatlas claude`, `quotatlas codex`) hands off within one provider. When the CLI exits on a limit, it asks, copies the transcript to the same relative path under the next account's home (`projects/<encoded cwd>/<id>.jsonl`, `sessions/<yyyy>/<mm>/<dd>/rollout-…-<id>.jsonl`) and resumes that session there with `--resume <id>` or `resume <id>`, sending a message that says the session moved. The next account is the untried one with the most room, unknown room after known room, full accounts never. Neither CLI documents that lookup; it was checked with Claude Code 2.1.273 and Codex 0.147.0.
- The CLI attaches shared memory per run (`--mcp-config`, or `-c mcp_servers.quotatlas.*` for Codex) and writes nothing into the CLI's own configuration.
- An interactive run is watched: when the transcript shows an account-level limit, the CLI is stopped (SIGINT, then SIGTERM, then SIGKILL, each to the CLI and its descendants in its process group, as Ctrl-C would reach them) and the terminal's saved `stty` state and modes are restored before the handoff question. A Claude limit on one model ("switch to another model") is left open.
- A resumed session keeps the first run's options. Which words are options and how many values each takes is read from the CLI's own `--help` at handoff time; prompts and session selectors are dropped. `claude -p` and `codex exec` stay in that mode.
- When no account of the CLI has room, the other CLI starts with a brief of the session: the first and latest requests, the last replies, and a pointer to `git status` and `memory_briefing`. A copied session the CLI cannot find is started over the same way on that account, after asking: the same exit is what declining Claude's folder-trust question looks like. A copy already in the target account that went its own way is kept as a `.bak` before the new one replaces it.

## Consequences

- No new process model. Accounts reuse the installed-agents map, its spawn path, restart and session reload.
- Two accounts of the same agent run as two processes.
- Claude quota for managed profiles lags until the profile has been used interactively; limit detection covers the gap.
- Quota readers never parse credential files, so an agent storage format change cannot break or leak anything in Quotatlas.
