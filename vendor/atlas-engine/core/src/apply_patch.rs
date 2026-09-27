// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use crate::function_tool::FunctionCallError;
use crate::safety::SafetyCheck;
use crate::safety::assess_patch_safety;
use crate::session::turn_context::TurnContext;
use crate::tools::sandboxing::ExecApprovalRequirement;
use crate::tools::sandboxing::executor_windows_sandbox_level;
use atlas_engine_apply_patch::ApplyPatchAction;
use atlas_engine_apply_patch::ApplyPatchFileChange;
use atlas_engine_protocol::models::PermissionProfile;
use atlas_engine_protocol::protocol::FileChange;
use atlas_engine_protocol::protocol::FileSystemSandboxPolicy;
use atlas_engine_utils_path_uri::PathUri;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct ApplyPatchRuntimeInvocation {
    pub(crate) action: ApplyPatchAction,
    pub(crate) auto_approved: bool,
    pub(crate) exec_approval_requirement: ExecApprovalRequirement,
}

pub(crate) fn prepare_apply_patch(
    turn_context: &TurnContext,
    permission_profile: &PermissionProfile,
    file_system_sandbox_policy: &FileSystemSandboxPolicy,
    action: ApplyPatchAction,
) -> Result<ApplyPatchRuntimeInvocation, FunctionCallError> {
    match assess_patch_safety(
        &action,
        turn_context.approval_policy(),
        permission_profile,
        file_system_sandbox_policy,
        &action.cwd,
        // Judge against the level the write would actually run under, not the
        // raw setting. `tools::runtimes::apply_patch` upgrades an unset level
        // for a Windows-shaped cwd before it builds the write's sandbox, so
        // reading the raw value here refused edits as unsandboxable that the
        // write path would in fact have contained. That mismatch is why
        // "Accept edits" prompted for every file edit on Windows.
        executor_windows_sandbox_level(turn_context.windows_sandbox_level, &action.cwd),
    ) {
        SafetyCheck::AutoApprove => Ok(ApplyPatchRuntimeInvocation {
            action,
            auto_approved: true,
            exec_approval_requirement: ExecApprovalRequirement::Skip {
                bypass_sandbox: false,
                proposed_execpolicy_amendment: None,
            },
        }),
        SafetyCheck::AskUser => {
            // Delegate the approval prompt (including cached approvals) to the
            // tool runtime, consistent with how shell/unified_exec approvals
            // are orchestrator-driven.
            Ok(ApplyPatchRuntimeInvocation {
                action,
                auto_approved: false,
                exec_approval_requirement: ExecApprovalRequirement::NeedsApproval {
                    reason: None,
                    proposed_execpolicy_amendment: None,
                },
            })
        }
        SafetyCheck::Reject { reason } => Err(FunctionCallError::RespondToModel(format!(
            "patch rejected: {reason}"
        ))),
    }
}

pub(crate) fn convert_apply_patch_to_protocol(
    action: &ApplyPatchAction,
) -> HashMap<PathBuf, FileChange> {
    let mut result = HashMap::with_capacity(action.changes().len());
    for (path, change) in action.changes() {
        let protocol_change = match change {
            ApplyPatchFileChange::Add { content, .. } => FileChange::Add {
                content: content.clone(),
            },
            ApplyPatchFileChange::Delete { content } => FileChange::Delete {
                content: content.clone(),
            },
            ApplyPatchFileChange::Update {
                unified_diff,
                move_path,
                new_content: _new_content,
            } => FileChange::Update {
                unified_diff: unified_diff.clone(),
                move_path: move_path.as_ref().map(PathUri::to_path_buf),
            },
        };
        // TODO(anp): Carry PathUri through patch protocol events once app-server and rollout
        // compatibility no longer require path-flavored strings.
        result.insert(path.to_path_buf(), protocol_change);
    }
    result
}

#[cfg(test)]
#[path = "apply_patch_tests.rs"]
mod tests;
