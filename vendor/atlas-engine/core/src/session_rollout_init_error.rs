// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::io::ErrorKind;
use std::path::Path;

use crate::rollout::SESSIONS_SUBDIR;
use atlas_engine_protocol::error::AtlasEngineErr;
use atlas_engine_thread_store::ThreadStoreError;

pub(crate) fn map_session_init_error(err: &anyhow::Error, atlas_agent_home: &Path) -> AtlasEngineErr {
    if let Some(store_error) = err
        .chain()
        .find_map(|cause| cause.downcast_ref::<ThreadStoreError>())
    {
        match store_error {
            ThreadStoreError::Unsupported { operation } => {
                return AtlasEngineErr::UnsupportedOperation(format!("{operation} is not supported yet"));
            }
            ThreadStoreError::Conflict { message } => {
                return AtlasEngineErr::InvalidRequest(message.clone());
            }
            ThreadStoreError::ThreadNotFound { .. }
            | ThreadStoreError::InvalidRequest { .. }
            | ThreadStoreError::Internal { .. } => {}
        }
    }

    if let Some(mapped) = err
        .chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .find_map(|io_err| map_rollout_io_error(io_err, atlas_agent_home))
    {
        return mapped;
    }

    AtlasEngineErr::Fatal(format!("Failed to initialize session: {err:#}"))
}

fn map_rollout_io_error(io_err: &std::io::Error, atlas_agent_home: &Path) -> Option<AtlasEngineErr> {
    let sessions_dir = atlas_agent_home.join(SESSIONS_SUBDIR);
    let hint = match io_err.kind() {
        ErrorKind::PermissionDenied => format!(
            "Atlas Agent cannot access session files at {} (permission denied). If sessions were created using sudo, fix ownership: sudo chown -R $(whoami) {}",
            sessions_dir.display(),
            atlas_agent_home.display()
        ),
        ErrorKind::NotFound => format!(
            "Session storage missing at {}. Create the directory or choose a different Atlas Agent home.",
            sessions_dir.display()
        ),
        ErrorKind::AlreadyExists => format!(
            "Session storage path {} is blocked by an existing file. Remove or rename it so Atlas Agent can create sessions.",
            sessions_dir.display()
        ),
        ErrorKind::InvalidData | ErrorKind::InvalidInput => format!(
            "Session data under {} looks corrupt or unreadable. Clearing the sessions directory may help (this will remove saved threads).",
            sessions_dir.display()
        ),
        ErrorKind::IsADirectory | ErrorKind::NotADirectory => format!(
            "Session storage path {} has an unexpected type. Ensure it is a directory Atlas Agent can use for session files.",
            sessions_dir.display()
        ),
        _ => return None,
    };

    Some(AtlasEngineErr::Fatal(format!(
        "{hint} (underlying error: {io_err})"
    )))
}
