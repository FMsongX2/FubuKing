// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::path::PathBuf;

use atlas_engine_utils_absolute_path::AbsolutePathBuf;

/// Runtime paths needed by exec-server child processes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecServerRuntimePaths {
    /// Stable path to the Atlas Agent executable used to launch hidden helper modes.
    pub atlas_engine_self_exe: AbsolutePathBuf,
    /// Path to the Linux sandbox helper alias used when the platform sandbox
    /// needs to re-enter Atlas Agent by argv0.
    pub atlas_engine_linux_sandbox_exe: Option<AbsolutePathBuf>,
}

impl ExecServerRuntimePaths {
    pub fn from_optional_paths(
        atlas_engine_self_exe: Option<PathBuf>,
        atlas_engine_linux_sandbox_exe: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        let atlas_engine_self_exe = atlas_engine_self_exe.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Atlas Agent executable path is not configured",
            )
        })?;
        Self::new(atlas_engine_self_exe, atlas_engine_linux_sandbox_exe)
    }

    pub fn new(
        atlas_engine_self_exe: PathBuf,
        atlas_engine_linux_sandbox_exe: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            atlas_engine_self_exe: absolute_path(atlas_engine_self_exe)?,
            atlas_engine_linux_sandbox_exe: atlas_engine_linux_sandbox_exe.map(absolute_path).transpose()?,
        })
    }
}

fn absolute_path(path: PathBuf) -> std::io::Result<AbsolutePathBuf> {
    AbsolutePathBuf::from_absolute_path(path.as_path())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))
}
