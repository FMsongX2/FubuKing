<!-- Modified by FubuKing from upstream Atlas (Apache-2.0). -->
<div align="center">

### FubuKing

**Every coding agent. Every account. One memory.**

Claude Code and Codex share one memory per repository, and a usage limit
becomes a handoff to your next account instead of a lost session.
It works wherever those CLIs run: a terminal, Orca, an IDE.

[Quick start](#quick-start) · [How it works](#how-it-works) · [Roadmap](#roadmap) · [Design records](docs/adr) · [Credits](#credits)

</div>

> [!WARNING]
> FubuKing is in early alpha. It is a fork of [Atlas](https://github.com/pacifio/atlas) that keeps Atlas's memory crates and leaves out its desktop app, hosted services and telemetry. The roadmap says what works now.

## Why

You pay for more than one coding agent, or more than one account of the same agent, and the limits hit at the worst moment.

- A five-hour or weekly limit stops a task halfway, and the context sits in a session you now have to abandon.
- Moving to another account or another agent means explaining the task again from zero.
- Nothing shows how much of each subscription is left until it is gone.

Agent workspaces such as Orca already run many agents side by side and switch accounts by hand. FubuKing is the layer underneath: one memory every agent reads and writes, and a session that carries on when an account runs out.

## Quick start

Install the CLI from a [release](https://github.com/FMsongX2/FubuKing/releases). Each archive comes with a signed build provenance attestation, which `gh attestation verify` checks against this repository's release workflow:

```bash
curl -fsSLO https://github.com/FMsongX2/FubuKing/releases/latest/download/fubuking-aarch64-apple-darwin.tar.gz
gh attestation verify fubuking-aarch64-apple-darwin.tar.gz -R FMsongX2/FubuKing --signer-workflow FMsongX2/FubuKing/.github/workflows/release-cli.yml
mkdir -p ~/.local/bin && tar -xzf fubuking-aarch64-apple-darwin.tar.gz -C ~/.local/bin fubuking
```

The other archives are `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `x86_64-pc-windows-msvc` (a `.zip`), and every archive holds `LICENSE`, `NOTICE` and `THIRD-PARTY-LICENSES.txt` beside the binary. The Linux ones need glibc 2.28 or newer (Ubuntu 20.04, Debian 10, RHEL 8 and later); building from source takes GCC 13 or newer, for the vector kernels under usearch. The macOS binaries carry no Apple Developer ID signature, so one downloaded in a browser needs `xattr -d com.apple.quarantine fubuking` first. Or build it (Rust stable via [rustup](https://rustup.rs/); the first build takes a few minutes):

```bash
cargo install --git https://github.com/FMsongX2/FubuKing fubuking --locked --root ~/.local --force
```

Either way it lands at `~/.local/bin/fubuking`; `--force` lets `cargo install` replace one already there.

Run your agents through it:

```bash
fubuking claude                 # Claude Code, on the account with the most room
fubuking codex                  # Codex, the same way
fubuking claude --account work  # a specific account; anything else is passed to the CLI
```

Add more accounts, each a separate official login:

```bash
fubuking login claude work      # opens Claude Code in a new profile: run /login there
fubuking login codex side       # runs `codex login` in a new profile
fubuking quota                  # what every account has left
fubuking doctor                 # what FubuKing reads here, when a handoff did not happen
```

### Memory in any MCP client

`fubuking claude` and `fubuking codex` attach the memory for that run only. To give every session the same memory, including agents another app starts, register the server once:

```bash
claude mcp add --scope user fubuking -- fubuking mcp --agent claude-code
codex mcp add fubuking -- fubuking mcp --agent codex
```

Codex asks before each tool call of a registered server; `default_tools_approval_mode = "approve"` under `[mcp_servers.fubuking]` in `~/.codex/config.toml` lets the memory tools run without asking, as they do under `fubuking codex`. Any other MCP client can run `fubuking mcp` as a stdio server from the repository directory.

## How it works

Memory. `fubuking mcp` serves seven tools: `memory_briefing`, `memory_changes`, `memory_search`, `memory_get`, `memory_list`, `memory_remember`, `memory_forget`. Agents record decisions, facts, failures and architecture notes; every later session, of any agent, starts from them. The record is one SQLite database per repository at `.atlas/memory/memory.sqlite`, shared by every worktree, and redacted before anything is written. To turn sharing off for a project, put `{"enabled": false}` in `.atlas/memory-sharing.json` in the folder you start the agent from.

Handoff. Claude Code and Codex both write a limit hit into the session transcript, and FubuKing watches it while the CLI runs. When an account runs out, it stops the CLI, asks whether to continue, copies the transcript to the next account's profile and resumes the same session there with `claude --resume` or `codex resume`, keeping the options you started with. The next account is the one with the most room left, read from the CLIs' own status line and app server. When no account of that CLI has room, the other CLI takes over with a brief of the session: what you asked, its last replies, and a pointer to the shared memory. An account that has not trusted the folder yet asks first, in either CLI, and FubuKing tells you which answer carries on before it starts; a newly signed-in account also asks its first-run questions once.

Accounts. Every account other than your default login is a profile directory, a separate `CLAUDE_CONFIG_DIR` or `CODEX_HOME` under FubuKing's config directory. Logins happen in the official CLIs, inside that profile, started by `fubuking login`.

Limits of the current version:

- The transcript layouts behind handoff are not documented by either CLI. They are tested with Claude Code 2.1.273 to 2.1.283 and Codex 0.147.0 to 0.157.1, in end-to-end tests that replay real records, and a CLI outside that range is announced once per version. When a CLI cannot find a copied session, FubuKing starts that account over from a brief. When a session ends on a limit in a form FubuKing does not read, or is saved where it does not look, nothing is handed off and FubuKing says so; `fubuking doctor` shows what it reads.
- A limit on one model, which Claude answers with "switch to another model", leaves the CLI open: the same account can go on with another model. Exit the CLI to hand off anyway.
- On Windows a running CLI is stopped with a console Ctrl-C, then ended with everything it started. A terminal that is not a Windows console, such as mintty, waits for the CLI to exit instead. An agent CLI that npm installed is a `.cmd` file, whose command line cannot hold a brief, so the brief goes into a temporary file the CLI is asked to read. CI compiles the Windows side and tests the process-tree kill; the console stop has not yet run on a real Windows console.
- FubuKing adds no status line when your own arguments decide the settings (`--settings`, `--setting-sources`, `--restricted`) or the project sets a status line; that run's Claude quota goes unread.

## Principles

- No telemetry. FubuKing has no analytics code and reports nothing.
- No hosted services. FubuKing never signs in to, syncs with or routes through Atlas's servers or any server of its own.
- Hands off your credentials. Logins happen in the official Claude Code and Codex CLIs. FubuKing never reads, copies or stores their tokens and never edits `~/.claude` or `~/.codex`; what it adds to a run, it passes as flags for that run, and your own status line runs as before.
- No silent account rotation. A handoff asks first.
- No account pool. Claude Code's [terms](https://code.claude.com/docs/en/legal-and-compliance) do not permit a third-party app to route requests through Free, Pro or Max plan credentials, so every account runs only in its own official CLI.

## Roadmap

- [x] Fork Atlas, rebrand, remove telemetry and hosted services
- [x] Accounts as profiles (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) and quota for each, read from the CLIs' own channels
- [x] `fubuking` CLI: shared memory over stdio MCP, limit detection and handoff to the next account
- [x] Handoff across agents: continue a Claude Code session in Codex, or the reverse, with a brief of the session
- [x] Hand off while the CLI is still open, keeping the first run's options
- [x] Prebuilt binaries for macOS, Linux and Windows with signed build provenance
- [ ] Apple Developer ID signing and notarization

## Credits

FubuKing stands on:

- [Atlas](https://github.com/pacifio/atlas) by Adib Mohsin, Apache-2.0. FubuKing is a modified version of it; see [NOTICE](NOTICE).

FubuKing is not affiliated with or endorsed by Atlas, Anthropic or OpenAI.

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
