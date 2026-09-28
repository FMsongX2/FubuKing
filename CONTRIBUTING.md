<!-- Modified by FubuKing from upstream Atlas (Apache-2.0). -->
# Contributing to FubuKing

FubuKing is a fork of [Atlas](https://github.com/pacifio/atlas) by Adib Mohsin and the Atlas contributors, used under the Apache License 2.0. It is building multi-account quota tracking for Claude Code and Codex on top of Atlas; nearly everything else is Atlas's work. The rules below keep the fork easy to merge with upstream and clear about what it changed.

Ask questions in [GitHub Discussions](https://github.com/FMsongX2/FubuKing/discussions). Report bugs with the feedback button in the app or the [issue forms](https://github.com/FMsongX2/FubuKing/issues/new/choose). Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md), never in a public issue.

## Build from source

You need [Bun](https://bun.sh/), [Rust](https://rustup.rs/) stable, and Xcode Command Line Tools on macOS or the MSVC build tools on Windows. Optionally, `mise install` picks up the Bun version pinned in `mise.toml`. To run Claude Code or Codex agents, install the `claude` or `codex` CLI. No API keys, `.env` file or account are needed.

```bash
git clone https://github.com/FMsongX2/FubuKing
cd FubuKing
bun install
bun run dev:app   # the desktop app
bun run dev       # the frontend alone, in a browser, on a mock backend
```

The first Rust build takes a few minutes. `bun run dev:app` hot-reloads the frontend; Rust changes need a restart.

`bun run dev` serves the app at `localhost:1420` with no Rust running: `src/dev/mock-backend/` answers every `invoke()` and `listen()` with fake data. Pick a state with `?scenario=<name>` (the list is in `src/dev/mock-backend/scenarios/index.ts`), and add a scenario when a screen needs one. The mock cannot fake native window features, and a browser does not render exactly like the app's WebKit view, so check UI changes in `bun run dev:app` before opening a PR.

## Tests

```bash
bun run test               # all frontend and repository tests
bun run test:contracts     # the repository contract tests in tests/, including the licence notices
bun run typecheck
bun run lint
bun run format:check
cargo test -p <crate>      # one crate under crates/
cargo test -p atlas --lib  # src-tauri
bun run test:rust          # crates/ and src-tauri, a subset of CI's Rust jobs
```

The pre-commit hook runs lint-staged, `bun run typecheck` and `bun run test:contracts`. New behaviour needs a test, and a bug fix needs a test that fails without it. Tests live next to the code they cover; tests that check the repository as a whole live in `tests/`.

## Fork rules

- Keep internal names. The `atlas-*` crates, the `.atlas/` project directory and the `atlas-agent` storage key stay as they are. Renaming them gains users nothing and makes every upstream merge conflict. Only user-facing surfaces carry the FubuKing name.
- Mark every upstream file you modify. Its first line, after a shebang or doctype if it has one, is `Modified by FubuKing from upstream Atlas (Apache-2.0).` in the file's comment syntax: `// ...` in Rust and TypeScript, `# ...` in YAML and shell, `<!-- ... -->` in Markdown and HTML. A file that cannot hold a comment, such as JSON, is listed in [FUBUKING-CHANGES.md](FUBUKING-CHANGES.md) instead. Files FubuKing adds need no notice. This is the change notice Apache-2.0 §4(b) requires; `tests/fubuking-notices.test.ts` enforces it against the fork base.
- Put new features in new files, and touch upstream files only where something has to be registered ([Q-0001](docs/adr/q0001-fubuking-fork-scope.md)).
- Write code comments in English only.
- Record decisions as ADRs in `docs/adr/`. FubuKing ADRs are Q-numbered (`q0001-...`, `q0002-...`; the next is `q0003-...`). Plain-numbered ADRs come from Atlas and keep their numbers.
- FubuKing sends no telemetry and uses no hosted services ([TELEMETRY.md](TELEMETRY.md)). A change that adds a network request lists it there.

Otherwise, follow the patterns already in the codebase: a feature folder under `src/features/<feature>/`, Zustand stores wrapped in `createSelectors`, Tailwind composed through `cn()`, and IPC verbs grouped into one `commands/<domain>.rs`. [ARCHITECTURE.md](ARCHITECTURE.md) describes how the layers fit together.

## Pull requests

Fork the repository, branch from `main`, and open the PR against `main`. The [PR template](.github/PULL_REQUEST_TEMPLATE.md) has the checklist.

`.gitignore` ignores `*.md` apart from listed exceptions such as `docs/adr/*.md`, so a new Markdown file elsewhere needs `git add -f` or a new exception.

## Merging upstream Atlas releases

Atlas tags its releases `alpha-X.Y.Z`, and FubuKing takes them by merging the tag. Dependency updates arrive the same way; FubuKing runs no Dependabot, because its bump PRs would conflict with these merges.

```bash
git remote add upstream https://github.com/pacifio/atlas   # once
git fetch upstream --tags
git checkout -b atlas-X.Y.Z main
git merge alpha-X.Y.Z
```

Resolve conflicts so that every file FubuKing changed keeps its change and its notice. In the same merge, set `FORK_BASE` in `tests/fubuking-notices.test.ts` to the tag's commit (`git rev-parse 'alpha-X.Y.Z^{commit}'`); otherwise every file upstream changed reads as a FubuKing modification. Then run `bun run test:contracts`, which reports modified files without a notice and `FUBUKING-CHANGES.md` entries that no longer differ from upstream.

Land the merge on `main` as a merge commit, not a squash or rebase, so the tag's commit stays in `main`'s history. The notice test diffs against it.

## Credits

FubuKing is built on Atlas by Adib Mohsin and the Atlas contributors. See [NOTICE](NOTICE). Participation is covered by the [Code of Conduct](CODE_OF_CONDUCT.md).
