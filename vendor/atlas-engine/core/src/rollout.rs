// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use crate::config::Config;
pub use atlas_engine_rollout::ARCHIVED_SESSIONS_SUBDIR;
pub use atlas_engine_rollout::Cursor;
pub use atlas_engine_rollout::INTERACTIVE_SESSION_SOURCES;
pub use atlas_engine_rollout::RolloutRecorder;
pub use atlas_engine_rollout::RolloutRecorderParams;
pub use atlas_engine_rollout::SESSIONS_SUBDIR;
pub use atlas_engine_rollout::SessionMeta;
pub use atlas_engine_rollout::SortDirection;
pub use atlas_engine_rollout::ThreadItem;
pub use atlas_engine_rollout::ThreadSortKey;
pub use atlas_engine_rollout::ThreadsPage;
pub use atlas_engine_rollout::append_thread_name;
pub use atlas_engine_rollout::find_archived_thread_path_by_id_str;
#[deprecated(note = "use find_thread_path_by_id_str")]
pub use atlas_engine_rollout::find_conversation_path_by_id_str;
pub use atlas_engine_rollout::find_thread_meta_by_name_str;
pub use atlas_engine_rollout::find_thread_name_by_id;
pub use atlas_engine_rollout::find_thread_names_by_ids;
pub use atlas_engine_rollout::find_thread_path_by_id_str;
pub use atlas_engine_rollout::parse_cursor;
pub use atlas_engine_rollout::read_head_for_summary;
pub use atlas_engine_rollout::read_session_meta_line;
pub use atlas_engine_rollout::rollout_date_parts;

impl atlas_engine_rollout::RolloutConfigView for Config {
    fn atlas_agent_home(&self) -> &std::path::Path {
        self.atlas_agent_home.as_path()
    }

    fn sqlite_config(&self) -> &atlas_engine_state::SqliteConfig {
        self.sqlite_config()
    }

    fn cwd(&self) -> &std::path::Path {
        self.cwd.as_path()
    }

    fn model_provider_id(&self) -> &str {
        self.model_provider_id.as_str()
    }

    fn generate_memories(&self) -> bool {
        self.memories.generate_memories
    }
}

pub(crate) mod list {
    pub use atlas_engine_rollout::find_thread_path_by_id_str;
}

#[cfg(test)]
pub(crate) mod recorder {
    pub use atlas_engine_rollout::RolloutRecorder;
}

pub(crate) use crate::session_rollout_init_error::map_session_init_error;

pub(crate) mod truncation {
    pub(crate) use crate::thread_rollout_truncation::*;
}
