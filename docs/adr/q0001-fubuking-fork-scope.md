# Q-0001: FubuKing is a branded fork of Atlas with a quota layer

Status: Accepted (2026-09-27), amended 2026-09-28 to drop the account pool and 2026-09-29 to drop the desktop app

Q-numbered ADRs belong to FubuKing. Upstream Atlas's own ADRs described its desktop app and left with it.

## Context

- Atlas (pacifio/atlas, Apache-2.0) runs Claude Code, Codex and ACP agents in one window, with shared memory, session history and commit-linked checkpoints.
- Quotio (nguyenphutrong/quotio, MIT) and CLIProxyAPI (router-for-me/CLIProxyAPI, MIT) track subscription quotas and pool provider accounts behind a local proxy.
- No open-source tool does both. Users run two apps and wire them together by editing global agent config.

## Decision

- Fork Atlas at `alpha-0.3.4`. Keep the `upstream` remote and take upstream releases by merging them.
- Carry only the `fubuking` CLI and the Atlas crates it builds on (2026-09-29). The desktop app, its frontend, the vendored agent engine and the crates only the app used are removed. The product is the CLI (Q-0002). Planned, not designed: attaching FubuKing to Orca in place of an app of its own.
- Add features as new crates. Touch upstream files only at registration points.
- Rebrand only product-facing surfaces: the CLI, the config directory's identifier, docs, external URLs.
- Keep internal identifiers unchanged: `atlas-*` crate names and the `.atlas/` project directory. Renaming them buys nothing for users and breaks every upstream merge.
- Ship with zero telemetry. The CLI carries no analytics client.
- Call no Atlas-operated service. The features that did (organisations, account sync, hosted endpoints) left with the desktop app.

## Licensing

- The fork stays Apache-2.0. Upstream `LICENSE`, `NOTICE` files and copyright lines are kept; a FubuKing copyright line is added beside them, never in place of them.
- Every upstream file FubuKing modifies carries a first-line notice: `Modified by FubuKing from upstream Atlas (Apache-2.0).` A test enforces it against the fork base.
- "Atlas" is not used as a product name. FubuKing credits Atlas as its upstream in the README and NOTICE.

## Product rules

- Agent routing is per session. FubuKing injects account selection into the agent process environment it spawns; it never edits `~/.claude/settings.json` or `~/.codex/config.toml`.
- Account profiles use each agent's own login (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`).
- FubuKing never reads, copies or stores provider credentials. Login and token storage stay inside the official CLIs; quota data comes only from channels those CLIs expose (for example `codex app-server` rate-limit reads). FubuKing does not reuse official OAuth client ids.
- No account pool. Claude Code's terms do not permit a third-party app to route requests through Free, Pro or Max plan credentials or to intermediate their session tokens ([legal and compliance](https://code.claude.com/docs/en/legal-and-compliance), read 2026-09-28). A CLIProxyAPI pool does both, so the opt-in pool mode first planned here is dropped.
- No automatic rotation across subscription accounts: a handoff asks first.

## Consequences

- Upstream merges conflict at registration points and branded strings, and stop on every file FubuKing deleted; files upstream adds under removed paths come back and are deleted again. CONTRIBUTING describes taking a release.
- Internal names still read `atlas`; contributors see both names.
