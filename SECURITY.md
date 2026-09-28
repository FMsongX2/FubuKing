<!-- Modified by FubuMem from upstream Atlas (Apache-2.0). -->
# Security

Report security issues privately. Never open a public issue for a vulnerability.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting: open the repository's Security tab and choose "Report a vulnerability" (https://github.com/FMsongX2/FubuMem/security/advisories/new). Include:

- FubuMem version, from Settings → About.
- OS and version, and on a Mac whether it is Apple Silicon or Intel.
- Steps to reproduce.
- Impact: what an attacker could do with it.

If the problem is in code FubuMem inherits unchanged from Atlas, report it to Atlas as well, following [Atlas's security policy](https://github.com/pacifio/atlas/security/policy). Atlas users are affected too.

## Scope

FubuMem runs coding agents that read files and execute commands. An agent doing that after you approved it is the product working as designed. An agent doing it without the approval step, or reaching outside the project you granted it, is a vulnerability.

Credential handling, the updater, and anything that causes local data to leave the machine unexpectedly are all in scope. FubuMem is designed never to read, copy or store Claude Code or Codex credentials, never to send telemetry and never to contact Atlas's hosted services. A way to make it do any of these is a vulnerability.

## What to expect

We acknowledge reports as soon as we can and prioritise confirmed issues. There is no formal SLA, and response times vary. Please hold off on public disclosure until we have had a chance to look.

## Supported versions

FubuMem is pre-1.0. Only the latest release on `main` is supported. Update and confirm the issue still reproduces before reporting. Until the first release, report against the current `main`.
