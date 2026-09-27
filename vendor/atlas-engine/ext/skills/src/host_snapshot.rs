// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::io;
use std::sync::Arc;

use crate::SkillLoadOutcome;
use atlas_engine_skills::SkillMetadata;

/// Immutable snapshot of host-owned skills and their source filesystems.
#[derive(Debug, Clone)]
pub struct HostSkillsSnapshot {
    outcome: Arc<SkillLoadOutcome>,
}

impl HostSkillsSnapshot {
    pub fn new(outcome: Arc<SkillLoadOutcome>) -> Self {
        Self { outcome }
    }

    pub fn outcome(&self) -> &SkillLoadOutcome {
        self.outcome.as_ref()
    }

    pub async fn read_skill_text(&self, skill: &SkillMetadata) -> io::Result<String> {
        self.outcome.read_skill_text(skill).await
    }
}
