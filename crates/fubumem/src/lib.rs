//! FubuMem: one memory and usage-limit handoff for Claude Code, Codex and
//! any MCP agent. The `fubumem` binary and the desktop app share this crate.

pub mod accounts;
pub mod args;
pub mod mcp;
pub mod quota;
pub mod run;
pub mod sessions;
