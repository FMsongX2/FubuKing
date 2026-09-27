<!-- Modified by Quotatlas from upstream Atlas (Apache-2.0). -->
<div align="center">

### Quotatlas

**Every coding agent. Every account. One memory.**

Run Claude Code, Codex and any ACP agent side by side, across all of your subscriptions,
with shared memory and a history that survives every rate limit.

[Roadmap](#roadmap) · [Build from source](#build-from-source) · [Design records](docs/adr) · [Credits](#credits)

</div>

> [!WARNING]
> Quotatlas is in early alpha. It is a fork of [Atlas](https://github.com/pacifio/atlas) with its hosted services and telemetry removed, plus accounts and quota tracking. The roadmap says what works now.

## Why

You pay for more than one coding agent, or more than one account of the same agent, and the limits hit at the worst moment.

- A five-hour or weekly limit stops a task halfway. The context lives in a terminal you now have to abandon.
- Switching to another account or another agent means explaining the task again from zero.
- Nothing shows how much of each subscription is left until it is gone.

Atlas already solves the second half: agents run in one window, share memory, and every commit links back to the session that produced it. Quotio and CLIProxyAPI solve the first half: they track quotas and pool accounts. Quotatlas puts both in one app so hitting a limit becomes a handoff instead of a restart.

## What it does

From Atlas:

- Claude Code, Codex, a native agent and any agent from the ACP registry in one window.
- Shared memory: decisions, plans, failures and architecture notes follow you across agents.
- Checkpoints: every commit is linked to the session, prompts and tool calls that produced it.
- Editor, git graph, terminal, knowledge base and research tools around the agents.

Added by Quotatlas:

- Accounts as first-class agents: "Claude Code · work" and "Claude Code · personal" side by side, each with its own official login.
- Quota at a glance for every account, read only from channels the official CLIs expose.
- Limit handoff: when an account runs out, continue on another account or agent with the full context carried over.
- Optional local account pool through CLIProxyAPI, managed by the app.

## Principles

- No telemetry. Builds ship without an analytics key and the setting defaults to off.
- No hosted services. Quotatlas never signs in to, syncs with or routes through Atlas's servers or any server of its own.
- Hands off your credentials. Logins happen in the official Claude Code and Codex CLIs. Quotatlas never reads, copies or stores their tokens and never edits `~/.claude` or `~/.codex`.
- Automatic account rotation is off by default. Pooling subscription accounts can conflict with provider terms; Quotatlas labels that risk instead of hiding it.

## Roadmap

- [x] Fork Atlas, rebrand, install side by side with Atlas
- [x] Remove telemetry and hosted services
- [x] Account profiles as agent entries (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`), managed in Settings → Accounts
- [x] Quota tab and title-bar readout: Codex through its app server, Claude through its status line
- [ ] Limit detection and one-click handoff to another account or agent
- [ ] Managed CLIProxyAPI pool, and the native agent on your own subscriptions
- [ ] Signed release builds

## Build from source

Requirements: [Bun](https://bun.sh/), Rust stable via [rustup](https://rustup.rs/), and Xcode Command Line Tools on macOS or the MSVC build tools on Windows. To use Claude Code, install the `claude` CLI; for Codex, install the `codex` CLI.

```bash
git clone https://github.com/FMsongX2/Quotatlas
cd Quotatlas
bun install
bun run dev:app
```

The first Rust build takes a few minutes. `bun run test:contracts` runs the repository contract tests, including the licence checks.

## Credits

Quotatlas stands on:

- [Atlas](https://github.com/pacifio/atlas) by Adib Mohsin, Apache-2.0. Quotatlas is a modified version of it; see [NOTICE](NOTICE).
- [Quotio](https://github.com/nguyenphutrong/quotio), MIT, whose quota model, CLIProxyAPI lifecycle and quota screens informed the quota layer and its UI.
- [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI), MIT, downloaded at runtime and verified against its published checksum when the pool is enabled.

Quotatlas is not affiliated with or endorsed by Atlas, Anthropic or OpenAI.

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
