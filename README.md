<!-- Modified by Quotatlas from upstream Atlas (Apache-2.0). -->
<div align="center">

### Quotatlas

**Every coding agent. Every account. One memory.**

Claude Code and Codex share one memory per repository, and a usage limit
becomes a handoff to your next account instead of a lost session.
It works wherever those CLIs run: a terminal, Orca, an IDE.

[Quick start](#quick-start) · [How it works](#how-it-works) · [Roadmap](#roadmap) · [Design records](docs/adr) · [Credits](#credits)

</div>

> [!WARNING]
> Quotatlas is in early alpha. It is a fork of [Atlas](https://github.com/pacifio/atlas) with its hosted services and telemetry removed. The roadmap says what works now.

## Why

You pay for more than one coding agent, or more than one account of the same agent, and the limits hit at the worst moment.

- A five-hour or weekly limit stops a task halfway, and the context sits in a session you now have to abandon.
- Moving to another account or another agent means explaining the task again from zero.
- Nothing shows how much of each subscription is left until it is gone.

Agent workspaces such as Orca already run many agents side by side and switch accounts by hand. Quotatlas is the layer underneath: one memory every agent reads and writes, and a session that carries on when an account runs out.

## Quick start

Install the CLI (Rust stable via [rustup](https://rustup.rs/); the first build takes a few minutes):

```bash
cargo install --git https://github.com/FMsongX2/Quotatlas quotatlas --locked --root ~/.local --force
```

`--root ~/.local` puts it at `~/.local/bin/quotatlas`, where the desktop app's folder-opening helper would otherwise go; `--force` replaces that helper, and `quotatlas open` does its job.

Run your agents through it:

```bash
quotatlas claude                 # Claude Code, on the account with the most room
quotatlas codex                  # Codex, the same way
quotatlas claude --account work  # a specific account; anything else is passed to the CLI
```

Add more accounts, each a separate official login:

```bash
quotatlas login claude work      # opens Claude Code in a new profile: run /login there
quotatlas login codex side       # runs `codex login` in a new profile
quotatlas quota                  # what every account has left
```

### Memory in any MCP client

`quotatlas claude` and `quotatlas codex` attach the memory for that run only. To give every session the same memory, including agents another app starts, register the server once:

```bash
claude mcp add --scope user quotatlas -- quotatlas mcp --agent claude-code
codex mcp add quotatlas -- quotatlas mcp --agent codex
```

Codex asks before each tool call of a registered server; `default_tools_approval_mode = "approve"` under `[mcp_servers.quotatlas]` in `~/.codex/config.toml` lets the memory tools run without asking, as they do under `quotatlas codex`. Any other MCP client can run `quotatlas mcp` as a stdio server from the repository directory.

## How it works

Memory. `quotatlas mcp` serves seven tools: `memory_briefing`, `memory_changes`, `memory_search`, `memory_get`, `memory_list`, `memory_remember`, `memory_forget`. Agents record decisions, facts, failures and architecture notes; every later session, of any agent, starts from them. The record is one SQLite database per repository at `.atlas/memory/memory.sqlite`, shared by every worktree, redacted before anything is written, and the same record the desktop app uses.

Handoff. Claude Code and Codex both write a limit hit into the session transcript, and Quotatlas watches it while the CLI runs. When an account runs out, it stops the CLI, asks whether to continue, copies the transcript to the next account's profile and resumes the same session there with `claude --resume` or `codex resume`, keeping the options you started with. The next account is the one with the most room left, read from the CLIs' own status line and app server. When no account of that CLI has room, the other CLI takes over with a brief of the session: what you asked, its last replies, and a pointer to the shared memory.

Accounts. Every account other than your default login is a profile directory, a separate `CLAUDE_CONFIG_DIR` or `CODEX_HOME` under the app's config directory. Logins happen in the official CLIs, and the desktop app lists the same accounts.

Limits of the current version:

- The transcript layouts behind handoff are not documented by either CLI. Checked with Claude Code 2.1.273 and Codex 0.147.0 and 0.157.1. When a CLI cannot find a copied session, Quotatlas starts that account over from a brief; when the limit record itself changes shape, the limit goes unnoticed and the run just ends.
- A limit on one model, which Claude answers with "switch to another model", leaves the CLI open: the same account can go on with another model. Exit the CLI to hand off anyway.
- Windows waits for the CLI to exit before handing off.

## Principles

- No telemetry. Builds ship without an analytics key and the setting defaults to off.
- No hosted services. Quotatlas never signs in to, syncs with or routes through Atlas's servers or any server of its own.
- Hands off your credentials. Logins happen in the official Claude Code and Codex CLIs. Quotatlas never reads, copies or stores their tokens and never edits `~/.claude` or `~/.codex`; what it adds to a run, it passes as flags for that run, and your own status line runs as before.
- No silent account rotation. A handoff asks first. Pooling subscription accounts can conflict with provider terms; Quotatlas labels that risk instead of hiding it.

## The desktop app

The repository also builds a desktop app, the Atlas workspace with accounts and a quota tab added: agents in one window over the same memory, checkpoints linking every commit to the session that produced it, and an editor, terminal and knowledge base around them. It is optional; the CLI does not need it.

Requirements: [Bun](https://bun.sh/), Rust stable, and Xcode Command Line Tools on macOS or the MSVC build tools on Windows.

```bash
git clone https://github.com/FMsongX2/Quotatlas
cd Quotatlas
bun install
bun run dev:app
```

`bun run test:contracts` runs the repository contract tests, including the licence checks.

## Roadmap

- [x] Fork Atlas, rebrand, remove telemetry and hosted services
- [x] Accounts as profiles (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) and quota for each, read from the CLIs' own channels
- [x] `quotatlas` CLI: shared memory over stdio MCP, limit detection and handoff to the next account
- [x] Handoff across agents: continue a Claude Code session in Codex, or the reverse, with a brief of the session
- [x] Hand off while the CLI is still open, keeping the first run's options
- [x] The desktop app lists accounts made with the CLI
- [ ] Managed CLIProxyAPI pool, and the native agent on your own subscriptions
- [ ] Prebuilt, signed binaries

## Credits

Quotatlas stands on:

- [Atlas](https://github.com/pacifio/atlas) by Adib Mohsin, Apache-2.0. Quotatlas is a modified version of it; see [NOTICE](NOTICE).
- [Quotio](https://github.com/nguyenphutrong/quotio), MIT, whose quota model, CLIProxyAPI lifecycle and quota screens informed the quota layer and its UI.
- [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI), MIT, which the planned account pool will download at runtime and verify against its published checksum.

Quotatlas is not affiliated with or endorsed by Atlas, Anthropic or OpenAI.

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
