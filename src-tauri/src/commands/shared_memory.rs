// Modified by FubuMem from upstream Atlas (Apache-2.0).
//! Shared memory's Tauri face: the Shared-tab commands and the session
//! lifecycle hook. The facade itself, and everything the rest of the app
//! reaches through this module, is `atlas_shared_memory::store`, which
//! `fubumem mcp` serves too.

use std::sync::Arc;

use tauri::State;

pub use atlas_shared_memory::store::*;

impl super::agent_host::SessionLifecycle for SharedMemoryStore {
    fn session_started(&self, session_id: &str, agent: &str, cwd: &str) {
        SharedMemoryStore::session_started(self, session_id, agent, cwd);
    }

    fn session_ended(&self, session_id: &str) {
        let _ = SharedMemoryStore::session_ended(self, session_id);
    }
}

// ── Tauri commands ───────────────────────────────────────────────────────────
//
// Async so the record's disk I/O runs on the blocking pool, never on the
// Tauri main thread. Request and response shapes are unchanged.

async fn off_main<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn memory_get_state(
    project_path: String,
    store: State<'_, SharedMemoryStore>,
) -> Result<SharedState, String> {
    let store = store.inner().clone();
    off_main(move || Ok(store.get_state(&project_path))).await
}

#[tauri::command]
pub async fn memory_query(
    project_path: String,
    query: String,
    limit: Option<usize>,
    store: State<'_, SharedMemoryStore>,
) -> Result<Vec<MemoryEvent>, String> {
    let store = store.inner().clone();
    off_main(move || Ok(store.query(&project_path, &query, limit.unwrap_or(20)))).await
}

#[tauri::command]
pub async fn memory_list_events(
    project_path: String,
    store: State<'_, SharedMemoryStore>,
) -> Result<Vec<MemoryEvent>, String> {
    let store = store.inner().clone();
    off_main(move || Ok(store.list_events(&project_path))).await
}

#[tauri::command]
pub async fn memory_clear_project(
    project_path: String,
    store: State<'_, SharedMemoryStore>,
) -> Result<(), String> {
    let store = store.inner().clone();
    off_main(move || store.clear(&project_path)).await
}

/// Manual structured write — used by tests, the UI, and (later) an agent
/// write-tool. `kind` must be a snake_case [`EventKind`].
#[tauri::command]
pub async fn memory_append_event(
    project_path: String,
    agent: String,
    session_id: String,
    kind: EventKind,
    key: Option<String>,
    payload: serde_json::Value,
    store: State<'_, SharedMemoryStore>,
) -> Result<u64, String> {
    let store = store.inner().clone();
    off_main(move || {
        store.append_event(
            &project_path,
            RawEvent {
                agent,
                session_id,
                kind,
                key: key.unwrap_or_default(),
                payload,
            },
        )
    })
    .await
}

/// Every entry with its provenance and confidence — the Shared tab's
/// Memories view.
#[tauri::command]
pub async fn memory_list_entries(
    project_path: String,
    store: State<'_, SharedMemoryStore>,
) -> Result<Vec<MemoryEntry>, String> {
    let store = store.inner().clone();
    off_main(move || Ok(store.entries(&project_path))).await
}

/// Edit one entry's content as the user. The retrieval index is nudged so
/// relevant memory stops matching the old wording.
#[tauri::command]
pub async fn memory_edit_entry(
    project_path: String,
    id: i64,
    content: String,
    store: State<'_, SharedMemoryStore>,
    registry: State<'_, Arc<super::memory_indexer::MemoryRegistry>>,
) -> Result<MemoryEntry, String> {
    let store = store.inner().clone();
    let cwd = project_path.clone();
    let edited = off_main(move || store.edit_entry(&project_path, id, &content)).await?;
    registry.enqueue_index(&cwd);
    Ok(edited)
}

/// Forget (delete) one entry. `false` when it was already gone. The
/// retrieval index is nudged so relevant memory stops finding it.
#[tauri::command]
pub async fn memory_forget_entry(
    project_path: String,
    id: i64,
    store: State<'_, SharedMemoryStore>,
    registry: State<'_, Arc<super::memory_indexer::MemoryRegistry>>,
) -> Result<bool, String> {
    let store = store.inner().clone();
    let cwd = project_path.clone();
    let gone = off_main(move || store.forget_entry(&project_path, id)).await?;
    registry.enqueue_index(&cwd);
    Ok(gone)
}
