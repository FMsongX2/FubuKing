// Modified by Quotatlas from upstream Atlas (Apache-2.0).
//! The running server, and the app-level host that owns it.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::middleware;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::sync::oneshot;

use atlas_shared_memory::briefing::{SessionClocks, SessionReads};
use atlas_shared_memory::tools::{Caller, CallerOf, MemoryTools, SharingGate, Sources};
use rmcp::service::{RequestContext, RoleServer};

use super::tokens::{require_token, Grant, MemoryTokens};
use super::MCP_PATH;
use crate::commands::shared_memory::SharedMemoryStore;

/// The running server. Dropping it (or [`shutdown`](Self::shutdown)) stops it.
pub struct MemoryServer {
    addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
}

impl MemoryServer {
    /// Bind `127.0.0.1:0` and serve the memory tools over `memory`, admitting
    /// only requests bearing a live token from `tokens`. Returns once bound;
    /// serving continues on the runtime.
    pub async fn start(
        memory: SharedMemoryStore,
        tokens: Arc<MemoryTokens>,
        clocks: Arc<SessionClocks>,
        reads: Arc<SessionReads>,
        gate: SharingGate,
        sources: Sources,
    ) -> std::io::Result<Self> {
        Self::start_with(memory, tokens, clocks, reads, gate, sources, Vec::new()).await
    }

    /// [`start`](Self::start), also serving each of `mounts` — another MCP
    /// service already routed at its own path, such as the UI tool server at
    /// `/ui` — on the same listener and behind the same token check. One
    /// listener means one token per session covers every service: the token
    /// table holds one token per session, so a second would revoke the first.
    pub async fn start_with(
        memory: SharedMemoryStore,
        tokens: Arc<MemoryTokens>,
        clocks: Arc<SessionClocks>,
        reads: Arc<SessionReads>,
        gate: SharingGate,
        sources: Sources,
        mounts: Vec<axum::Router>,
    ) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let tools = MemoryTools::new(memory, gate, clocks, reads, sources, token_caller());
        let service = StreamableHttpService::new(
            move || Ok(tools.clone()),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
        let router = mounts
            .into_iter()
            .fold(axum::Router::new().nest_service(MCP_PATH, service), axum::Router::merge)
            // Last, so it wraps every mounted service too.
            .layer(middleware::from_fn_with_state(tokens, require_token));
        let (stop, stopped) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let served = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await;
            if let Err(e) = served {
                tracing::warn!(target: "atlas::memory_server", "memory tool server stopped: {e}");
            }
        });
        tracing::info!(target: "atlas::memory_server", "memory tool server on http://{addr}{MCP_PATH}");
        Ok(Self { addr, stop: Some(stop) })
    }

    /// The MCP endpoint, e.g. `http://127.0.0.1:53124/mcp`.
    pub fn url(&self) -> String {
        format!("http://{}{MCP_PATH}", self.addr)
    }

    /// The endpoint of a service mounted at `path`, e.g. `url_at("/ui")`.
    pub fn url_at(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// Stop serving.
    pub fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

impl Drop for MemoryServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The app's one memory tool server, its tokens and its per-session clocks,
/// as managed state. Tokens and clocks exist from the start; the server's
/// URL once it has bound.
#[derive(Default)]
pub struct MemoryServerHost {
    tokens: Arc<MemoryTokens>,
    clocks: Arc<SessionClocks>,
    reads: Arc<SessionReads>,
    server: std::sync::OnceLock<MemoryServer>,
}

impl MemoryServerHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// The live session tokens (minted and revoked by the session lifecycle).
    pub fn tokens(&self) -> &Arc<MemoryTokens> {
        &self.tokens
    }

    /// Each session's "last looked" clock (dropped by the session lifecycle
    /// when the session ends).
    pub fn clocks(&self) -> &Arc<SessionClocks> {
        &self.clocks
    }

    /// Which sessions have read memory. Only the tests ask any more; the
    /// dispatcher still records reads so that distinction stays pinned.
    #[cfg(test)]
    pub fn reads(&self) -> &Arc<SessionReads> {
        &self.reads
    }

    /// The MCP endpoint, once the server has bound; `None` before that or if
    /// binding failed (sessions then run without memory tools).
    pub fn url(&self) -> Option<String> {
        self.server.get().map(MemoryServer::url)
    }

    /// The endpoint of a service mounted at `path`, once the server has bound.
    pub fn url_at(&self, path: &str) -> Option<String> {
        self.server.get().map(|s| s.url_at(path))
    }

    /// Start the server on the async runtime, with `mounts` served beside the
    /// memory tools; returns at once. A failure to bind is logged and leaves
    /// [`url`](Self::url) `None`.
    pub fn start(
        self: &Arc<Self>,
        memory: SharedMemoryStore,
        gate: SharingGate,
        sources: Sources,
        mounts: Vec<axum::Router>,
    ) {
        let host = self.clone();
        tauri::async_runtime::spawn(async move {
            let started = MemoryServer::start_with(
                memory,
                host.tokens.clone(),
                host.clocks.clone(),
                host.reads.clone(),
                gate,
                sources,
                mounts,
            )
            .await;
            match started {
                Ok(server) => {
                    let _ = host.server.set(server);
                }
                Err(e) => tracing::warn!(target: "atlas::memory_server", "memory tool server did not start: {e}"),
            }
        });
    }

    /// A server that is already running, for tests.
    #[cfg(test)]
    pub(crate) fn adopt(&self, server: MemoryServer) {
        let _ = self.server.set(server);
    }
}

/// The caller behind an HTTP request: the session its bearer token was
/// minted for, which `require_token` put on the request.
fn token_caller() -> CallerOf {
    Arc::new(|context: &RequestContext<RoleServer>| {
        let parts = context.extensions.get::<axum::http::request::Parts>()?;
        let grant = parts.extensions.get::<Grant>()?;
        Some(Caller { session_id: grant.session_id.clone(), agent: grant.agent.clone(), cwd: grant.cwd.clone() })
    })
}
