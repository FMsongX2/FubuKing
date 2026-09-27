#![allow(clippy::expect_used)]
// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.

// Single integration test binary that aggregates all test modules.
// The submodules live in `tests/all/`.
pub use atlas_engine_protocol::error;

mod suite;
