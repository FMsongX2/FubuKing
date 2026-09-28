//! FubuKing: one memory and usage-limit handoff for Claude Code, Codex and
//! any MCP agent. The `fubuking` binary and the desktop app share this crate.

pub mod accounts;
pub mod args;
pub mod mcp;
pub mod quota;
pub mod run;
pub mod sessions;

/// `program` as the OS can start it. On Windows that takes a `PATH` search
/// with `PATHEXT`, which `Command` does not do, and npm installs the agent
/// CLIs there as `.cmd` files. Elsewhere, or when nothing is found, the name,
/// for `Command` to look up and report.
pub fn executable(program: &str) -> std::path::PathBuf {
    #[cfg(windows)]
    if let Ok(path) = which::which(program) {
        return path;
    }
    std::path::PathBuf::from(program)
}
