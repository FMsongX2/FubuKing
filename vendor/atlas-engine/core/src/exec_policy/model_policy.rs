// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use super::ExecPolicyManager;
use atlas_engine_execpolicy::Decision;
use atlas_engine_execpolicy::Policy;
use atlas_engine_execpolicy::PrefixRule;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AllowPrefixRules {
    Honor,
    IgnoreForCyberModel,
}

impl ExecPolicyManager {
    pub(crate) fn current_for_prefix_rules(
        &self,
        allow_prefix_rules: AllowPrefixRules,
    ) -> Arc<Policy> {
        let policy = self.current();
        if allow_prefix_rules == AllowPrefixRules::Honor {
            return policy;
        }

        let rules = policy
            .rules()
            .iter_all()
            .flat_map(|(program, rules)| {
                rules.iter().filter_map(move |rule| {
                    let is_allow_prefix = rule
                        .as_any()
                        .downcast_ref::<PrefixRule>()
                        .is_some_and(|prefix| prefix.decision == Decision::Allow);
                    (!is_allow_prefix).then(|| (program.clone(), Arc::clone(rule)))
                })
            })
            .collect();

        Arc::new(Policy::from_parts(
            rules,
            policy.network_rules().to_vec(),
            policy.host_executables().clone(),
        ))
    }
}

#[cfg(test)]
#[path = "model_policy_tests.rs"]
mod tests;
