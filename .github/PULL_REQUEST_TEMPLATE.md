<!-- Modified by FubuMem from upstream Atlas (Apache-2.0). -->
<!--
Thanks for the PR. PRs target `main`. CONTRIBUTING.md has the build steps,
the tests and the fork rules.
-->

### What?

### Why?

### How?

Fixes #

## Checklist

- [ ] `bun run test` and `bun run test:contracts` pass
- [ ] The relevant `cargo test` passes for Rust changes (`cargo test -p <crate>`, or `cargo test -p atlas --lib` for `src-tauri`)
- [ ] New behaviour has a test; a bug fix has a test that fails without it
- [ ] Code comments are in English only
- [ ] Every upstream Atlas file this modifies carries the notice `Modified by FubuMem from upstream Atlas (Apache-2.0).` in a comment on its first line, or is listed in `FUBUMEM-CHANGES.md` if it cannot hold a comment
- [ ] A design decision this makes is recorded as a Q-numbered ADR in `docs/adr/` (for example `q0003-short-title.md`)
