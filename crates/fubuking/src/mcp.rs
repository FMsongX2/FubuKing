//! `fubuking mcp`: the shared-memory tools over stdio, for one session of any
//! MCP agent. Every session serves the same tools over the same record
//! (`atlas_shared_memory`), so an agent started from a terminal, from Orca or
//! from an IDE reads and writes one memory per repository.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use atlas_shared_memory::briefing::{SessionClocks, SessionReads};
use atlas_shared_memory::store::SharedMemoryStore;
use atlas_shared_memory::tools::{Caller, MemoryTools, Sources};
use rmcp::ServiceExt;

/// The name the server goes by in each agent's MCP configuration.
pub const SERVER_NAME: &str = "fubuking";

/// Serve until the agent closes stdin. `agent` is recorded as the writer of
/// everything this session remembers.
pub async fn serve(cwd: &Path, agent: &str) -> anyhow::Result<()> {
    let cwd = cwd.to_string_lossy().into_owned();
    // The agent does not say which of its sessions started this server, so
    // the server names its own: one process serves one session.
    let session_id = format!("mcp-{}-{}", std::process::id(), crate::quota::now_secs());
    let memory = SharedMemoryStore::new();
    memory.session_started(&session_id, agent, &cwd);
    let caller = Caller { session_id: session_id.clone(), agent: agent.to_string(), cwd };
    let tools = MemoryTools::new(
        memory.clone(),
        Arc::new(sharing_enabled),
        Arc::new(SessionClocks::default()),
        Arc::new(SessionReads::default()),
        Sources::default(),
        Arc::new(move |_| Some(caller.clone())),
    );
    let service = tools.serve(rmcp::transport::stdio()).await.context("starting the MCP session")?;
    let _ = service.waiting().await;
    memory.session_ended(&session_id);
    Ok(())
}

/// The project's switch, `.atlas/memory-sharing.json`: sharing is on unless
/// it says `{"enabled": false}`.
// ponytail: read at the launch directory only; a session started in a
// subfolder of the project does not see the switch.
fn sharing_enabled(cwd: &str) -> bool {
    std::fs::read_to_string(Path::new(cwd).join(".atlas/memory-sharing.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|file| file.get("enabled").and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

/// How an agent CLI launches this server: the running binary, so the agent
/// starts the same build that started it.
pub fn server_command(agent: &str) -> (String, Vec<String>) {
    let program = std::env::current_exe()
        .map(|exe| exe.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "fubuking".to_string());
    (program, vec!["mcp".into(), "--agent".into(), agent.into()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharing_is_on_unless_the_project_switched_it_off() {
        let project = tempfile::tempdir().unwrap();
        let cwd = project.path().to_string_lossy().into_owned();
        assert!(sharing_enabled(&cwd));
        std::fs::create_dir_all(project.path().join(".atlas")).unwrap();
        std::fs::write(project.path().join(".atlas/memory-sharing.json"), r#"{ "enabled": false }"#).unwrap();
        assert!(!sharing_enabled(&cwd));
    }
}
