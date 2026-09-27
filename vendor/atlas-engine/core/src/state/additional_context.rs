// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use std::collections::BTreeMap;

use crate::context::AdditionalContextDeveloperFragment;
use crate::context::AdditionalContextUserFragment;
use crate::context::ContextualUserFragment;
use atlas_engine_protocol::models::ResponseInputItem;
use atlas_engine_protocol::protocol::AdditionalContextEntry;
use atlas_engine_protocol::protocol::AdditionalContextKind;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AdditionalContextStore {
    values: BTreeMap<String, AdditionalContextEntry>,
}

impl AdditionalContextStore {
    pub(crate) fn merge(
        &mut self,
        values: BTreeMap<String, AdditionalContextEntry>,
    ) -> Vec<ResponseInputItem> {
        let fragments = values
            .iter()
            .filter(|(key, value)| self.values.get(*key) != Some(*value))
            .map(|(key, entry)| match entry.kind {
                AdditionalContextKind::Untrusted => {
                    AdditionalContextUserFragment::new(key.clone(), entry.value.clone())
                        .into_response_input_item()
                }
                AdditionalContextKind::Application => {
                    AdditionalContextDeveloperFragment::new(key.clone(), entry.value.clone())
                        .into_response_input_item()
                }
            })
            .collect();
        self.values = values;
        fragments
    }
}
