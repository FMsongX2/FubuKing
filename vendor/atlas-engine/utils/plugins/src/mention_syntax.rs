// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
//! Sigils for tool/plugin mentions in plaintext (shared across Atlas Agent crates).

/// Default plaintext sigil for tools.
pub const TOOL_MENTION_SIGIL: char = '$';

/// Plugins use `@` in linked plaintext outside TUI.
pub const PLUGIN_TEXT_MENTION_SIGIL: char = '@';
