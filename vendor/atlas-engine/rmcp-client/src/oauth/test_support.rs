// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::path::Path;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;
use std::sync::PoisonError;

use tempfile::tempdir;

/// Serializes tests that mutate process-wide ATLAS_AGENT_HOME.
///
/// Keep OAuth tests on this one guard instead of defining per-module helpers; otherwise
/// concurrently running test modules can point File/Secrets storage at different homes.
pub(super) struct TempAtlasAgentHome {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
}

impl TempAtlasAgentHome {
    pub(super) fn new() -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let dir = tempdir().expect("create ATLAS_AGENT_HOME temp dir");
        unsafe {
            std::env::set_var("ATLAS_AGENT_HOME", dir.path());
        }
        Self {
            _guard: guard,
            _dir: dir,
        }
    }

    pub(super) fn path(&self) -> &Path {
        self._dir.path()
    }
}

impl Drop for TempAtlasAgentHome {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("ATLAS_AGENT_HOME");
        }
    }
}
