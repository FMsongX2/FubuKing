<!-- Modified by FubuKing from upstream Atlas (Apache-2.0). -->
# Contributing to FubuKing

FubuKing is a fork of [Atlas](https://github.com/pacifio/atlas) by Adib Mohsin and the Atlas contributors, used under the Apache License 2.0. It keeps Atlas's memory crates and builds the `fubuking` CLI on them: one shared memory and usage-limit handoff for Claude Code and Codex. The rules below keep it clear about what it changed and able to take upstream fixes.

Ask questions in [GitHub Discussions](https://github.com/FMsongX2/FubuKing/discussions). Report bugs with the [issue forms](https://github.com/FMsongX2/FubuKing/issues/new/choose). Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md), never in a public issue.

## Build from source

You need [Rust](https://rustup.rs/) stable, and Xcode Command Line Tools on macOS or the MSVC build tools on Windows. On Linux the vector kernels under usearch need GCC 13 or newer. To try a change end to end, install the `claude` or `codex` CLI.

```bash
git clone https://github.com/FMsongX2/FubuKing
cd FubuKing
cargo build -p fubuking   # target/debug/fubuking
```

The first build takes a few minutes.

## Tests

```bash
cargo test --workspace                                  # every crate, and the licence notices
cargo test -p <crate>                                   # one crate under crates/
cargo test -p fubuking --test handoff                   # whole handoffs on a terminal, with fake CLIs (Unix)
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs every crate's tests and clippy on Linux, and the CLI's on Windows as well. The handoff tests replay records of real Claude Code and Codex sessions (`crates/fubuking/tests/fixtures/`); when a CLI release changes them, replace those with the new version's, keeping every field and replacing ids, paths and text, and the tests show whether handoff still follows. New behaviour needs a test, and a bug fix needs a test that fails without it. Tests live next to the code they cover.

## Fork rules

- Keep internal names. The `atlas-*` crates and the `.atlas/` project directory stay as they are. Renaming them gains users nothing and makes every upstream merge conflict. Only user-facing surfaces carry the FubuKing name.
- Mark every upstream file you modify. Its first line, after a shebang or doctype if it has one, is `Modified by FubuKing from upstream Atlas (Apache-2.0).` in the file's comment syntax: `// ...` in Rust, `# ...` in TOML, YAML and shell, `<!-- ... -->` in Markdown. A file that cannot hold a comment, such as JSON, is listed in [FUBUKING-CHANGES.md](FUBUKING-CHANGES.md) instead. Files FubuKing adds need no notice. This is the change notice Apache-2.0 §4(b) requires; `crates/fubuking/tests/apache_notices.rs` enforces it against the fork base.
- Put new features in new files, and touch upstream files only where something has to be registered ([Q-0001](docs/adr/q0001-fubuking-fork-scope.md)).
- Write code comments in English only.
- Record decisions as ADRs in `docs/adr/`. FubuKing ADRs are Q-numbered (`q0001-...`, `q0002-...`; the next is `q0003-...`).
- FubuKing sends no telemetry and uses no hosted services (README, Principles). A change that adds a network request says so there.

Otherwise, follow the patterns already in the crate you touch.

## Pull requests

Fork the repository, branch from `main`, and open the PR against `main`. The [PR template](.github/PULL_REQUEST_TEMPLATE.md) has the checklist.

`.gitignore` ignores `*.md` apart from listed exceptions such as `docs/adr/*.md`, so a new Markdown file elsewhere needs `git add -f` or a new exception.

## Taking upstream Atlas releases

FubuKing carries only part of Atlas: the crates under `crates/` that the CLI builds on, and a few files at the root. Atlas tags its releases `alpha-X.Y.Z`, and FubuKing takes one by merging the tag and removing again what it does not carry. Dependency updates arrive the same way; FubuKing runs no Dependabot, because its bump PRs would conflict with these merges.

```bash
git remote add upstream https://github.com/pacifio/atlas   # once
git fetch upstream --tags
git checkout -b atlas-X.Y.Z main
git merge alpha-X.Y.Z
```

The merge stops on files upstream changed and FubuKing deleted: delete them again with `git rm`. It also adds, without a conflict, files upstream created under paths FubuKing does not carry, such as `src/`, `src-tauri/`, `vendor/` and crates the CLI does not build on: delete those too. Keep FubuKing's changes and notices in the rest. In the same merge, set `FORK_BASE` in `crates/fubuking/tests/apache_notices.rs` to the tag's commit (`git rev-parse 'alpha-X.Y.Z^{commit}'`); otherwise every file upstream changed reads as a FubuKing modification. Then `cargo test --workspace` reports modified files without a notice and `FUBUKING-CHANGES.md` entries that no longer differ from upstream.

Land the merge on `main` as a merge commit, not a squash or rebase, so the tag's commit stays in `main`'s history. The notice test diffs against it.

## Credits

FubuKing is built on Atlas by Adib Mohsin and the Atlas contributors. See [NOTICE](NOTICE). Participation is covered by the [Code of Conduct](CODE_OF_CONDUCT.md).
