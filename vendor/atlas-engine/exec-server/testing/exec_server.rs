// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
//! Minimal exec-server fixture for Bazel-only integration tests.
//!
//! Linking only exec-server avoids depending on the full Atlas Agent CLI binary
//! when a test only needs a WebSocket executor endpoint. It handles the arg0
//! helper mode because sandboxed process requests re-exec this binary.

use atlas_engine_exec_server::ExecServerRuntimePaths;
use atlas_engine_http_client::HttpClientFactory;
use atlas_engine_http_client::OutboundProxyPolicy;
#[cfg(unix)]
use std::ffi::OsStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(unix)]
    {
        let mut args = std::env::args_os();
        let _ = args.next();
        if args.next().as_deref()
            == Some(OsStr::new(
                atlas_engine_exec_server::ATLAS_AGENT_ARG0_EXEC_HELPER_ARG1,
            ))
        {
            atlas_engine_exec_server::run_arg0_exec_helper_main();
        }
    }

    let current_exe = std::env::current_exe()?;
    let runtime_paths =
        ExecServerRuntimePaths::new(current_exe, /*atlas_engine_linux_sandbox_exe*/ None)?;
    atlas_engine_exec_server::run_main(
        "ws://127.0.0.1:0",
        runtime_paths,
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
    )
    .await
}
