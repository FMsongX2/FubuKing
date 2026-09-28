# Q-0001: FubuMem is a branded fork of Atlas with a quota layer

Status: Accepted (2026-09-27)

Q-numbered ADRs belong to FubuMem. Plain-numbered ADRs are inherited from upstream Atlas and keep their numbers so upstream merges stay clean.

## Context

- Atlas (pacifio/atlas, Apache-2.0) runs Claude Code, Codex and ACP agents in one window, with shared memory, session history and commit-linked checkpoints.
- Quotio (nguyenphutrong/quotio, MIT) and CLIProxyAPI (router-for-me/CLIProxyAPI, MIT) track subscription quotas and pool provider accounts behind a local proxy.
- No open-source tool does both. Users run two apps and wire them together by editing global agent config.

## Decision

- Fork Atlas at `alpha-0.3.4`. Keep the `upstream` remote and merge upstream releases.
- Add quota features as new crates and new frontend features. Touch upstream files only at registration points.
- Rebrand only product-facing surfaces: app name, bundle identifier, window titles, icons, CLI helper, docs, update feed, external URLs.
- Keep internal identifiers unchanged: `atlas-*` crate names, the `.atlas/` project directory, the `atlas-agent` storage key. Renaming them buys nothing for users and breaks every upstream merge.
- Ship with zero telemetry. Builds carry no PostHog key, so the client stays inert.
- Disable features that call Atlas-operated services (organisations, account sync, hosted endpoints) until FubuMem has its own or the user points them elsewhere.

## Licensing

- The fork stays Apache-2.0. Upstream `LICENSE`, `NOTICE` files and copyright lines are kept; a FubuMem copyright line is added beside them, never in place of them.
- Every upstream file FubuMem modifies carries a first-line notice: `Modified by FubuMem from upstream Atlas (Apache-2.0).` A test enforces it against the fork base.
- Code ported from Quotio keeps its MIT notice in the file header and in `NOTICE`.
- CLIProxyAPI is not vendored. It is downloaded at runtime from its GitHub releases and verified against the published SHA-256 digest.
- "Atlas" is not used as a product name. FubuMem credits Atlas as its upstream in the README and About screen.

## Product rules

- Agent routing is per session. FubuMem injects account selection into the agent process environment it spawns; it never edits `~/.claude/settings.json` or `~/.codex/config.toml`.
- Account profiles use each agent's own login (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`). The CLIProxyAPI pool is an opt-in mode.
- FubuMem never reads, copies or stores provider credentials. Login and token storage stay inside the official CLIs; quota data comes only from channels those CLIs expose (for example `codex app-server` rate-limit reads). FubuMem does not reuse official OAuth client ids.
- In pool mode the credentials belong to CLIProxyAPI's own auth directory. FubuMem manages the process, not the tokens.
- Automatic rotation across subscription accounts is off by default and labelled with the provider terms risk.

## Consequences

- Upstream merges conflict only at registration points and branded strings.
- Internal names still read `atlas`; contributors see both names.
- The quota layer works without the proxy, so a broken CLIProxyAPI release does not break agents.
