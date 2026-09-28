//! Shared cross-agent memory, independent of any host.
//!
//! - [`store`]: the facade over each scope's record (`atlas_memory::record`).
//! - [`briefing`]: what a session pulls first, and what changed since.
//! - [`tools`]: the seven MCP tools and their instructions, as an rmcp
//!   handler any transport can serve.
//!
//! The desktop app serves the tools over loopback HTTP; `quotatlas mcp` serves
//! them over stdio. Both write the same record, so every agent on a
//! repository shares one memory whichever way it was started.

pub mod briefing;
pub mod store;
pub mod tools;
