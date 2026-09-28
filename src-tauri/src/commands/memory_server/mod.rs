// Modified by FubuKing from upstream Atlas (Apache-2.0).
//! The **memory tool server**: shared memory as MCP tools, served by the
//! Tauri backend itself over streamable HTTP on loopback. It is the only way
//! memory reaches an agent — nothing is prepended to a prompt (ADR-0010).
//! The tools themselves live in `atlas_shared_memory::tools`, which
//! `fubuking mcp` serves over stdio to agents the app did not start.
//!
//! - **One server per app**, bound to `127.0.0.1` on a port the OS picks. The
//!   app starts it at setup on the async runtime, so the main thread never
//!   waits on it ([`MemoryServerHost::start`]).
//! - **One bearer token per (session, scope)** ([`tokens`]): minted when a
//!   session starts, revoked when it ends, checked on every request. The
//!   token says who is calling and which scope's record to open.
//! - **Handed to every agent that can take it** ([`offers`]): ACP agents that
//!   advertise `mcpCapabilities.http`, and the native agent through its
//!   thread's engine config.
//! - **Seven tools** ([`tools`]), read first, write last: `memory_briefing`,
//!   `memory_changes`, `memory_search`, `memory_get`, `memory_list`,
//!   `memory_remember`, `memory_forget`. The server's instructions
//!   ([`INSTRUCTIONS`]) tell the agent when to call each — the briefing first
//!   in every session — because with nothing pushed, the protocol is what
//!   makes memory reach the model.
//! - **What a session has seen** ([`briefing`]): a per-session clock behind
//!   `memory_changes`, so a resumed agent asks for the delta rather than the
//!   whole record again.
//! - **Failure is "no memory", never a crash.** A read that fails returns an
//!   empty result; a write that fails returns a tool error the agent can read.

mod host;
mod offers;
#[cfg(test)]
mod tests;
mod tokens;

// The module's surface. The app wires `MemoryServerHost`, `MemorySessionOffers`
// and the source types; the rest is reached through the host or by the tests.
#[allow(unused_imports)]
pub use atlas_shared_memory::briefing::{SessionClocks, SessionReads};
#[allow(unused_imports)]
pub use host::{MemoryServer, MemoryServerHost};
#[allow(unused_imports)]
pub use offers::{MemorySessionOffers, OfferDecision};
#[allow(unused_imports)]
pub use tokens::{Grant, MemoryTokens};
/// How long an agent may cache a `tools/list` answer; the UI tool server
/// answers under the same rule.
pub(crate) use atlas_shared_memory::tools::TOOLS_LIST_TTL_MS;
#[allow(unused_imports)]
pub use atlas_shared_memory::tools::{
    Bootstrap, BootstrapSource, IndexDoc, IndexEvict, IndexSearch, SharingGate, Sources, INSTRUCTIONS,
};

/// The path the MCP endpoint is served at.
pub const MCP_PATH: &str = "/mcp";

/// The name the server goes by in every agent's MCP configuration.
pub const MEMORY_SERVER_NAME: &str = "atlas_memory";
