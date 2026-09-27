// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
//! Command parsing and safety utilities shared across Atlas Agent crates.

pub mod shell_detect;

pub mod bash;
pub(crate) mod command_safety;
pub mod parse_command;
pub mod powershell;

pub use command_safety::is_dangerous_command;
pub use command_safety::is_safe_command;
