// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
//! Read-path helpers for Atlas Agent memories.
//!
//! This crate owns memory injection, memory citation parsing, and telemetry
//! classification for read access to the memory folder. It intentionally does
//! not depend on the memory write pipeline.

pub mod citations;
mod metrics;
pub mod usage;

use atlas_engine_utils_absolute_path::AbsolutePathBuf;

pub fn memory_root(atlas_agent_home: &AbsolutePathBuf) -> AbsolutePathBuf {
    atlas_agent_home.join("memories")
}
