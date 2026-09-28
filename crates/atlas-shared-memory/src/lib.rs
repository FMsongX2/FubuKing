//! Shared cross-agent memory, independent of any host.
//!
//! - [`store`]: the facade over each scope's record (`atlas_memory::record`).
//! - [`briefing`]: what a session pulls first, and what changed since.
//! - [`tools`]: the seven MCP tools and their instructions, as an rmcp
//!   handler any transport can serve.
//!
//! `fubuking mcp` serves the tools over stdio, one process per session. Every
//! session writes the same record, so every agent on a repository shares one
//! memory whichever way it was started.

pub mod briefing;
pub mod store;
pub mod tools;
