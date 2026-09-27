<!-- Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md. -->
# Windows exec-server fixture

This directory contains the small Windows exec-server binary used by
foreign-OS tests. It links only `atlas-engine-exec-server` because the full Atlas Agent
Windows graph does not yet cross-build with Bazel.
