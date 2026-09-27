// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
use atlas_engine_otel::GOAL_BLOCKED_METRIC;
use atlas_engine_otel::GOAL_BUDGET_LIMITED_METRIC;
use atlas_engine_otel::GOAL_COMPLETED_METRIC;
use atlas_engine_otel::GOAL_CREATED_METRIC;
use atlas_engine_otel::GOAL_DURATION_SECONDS_METRIC;
use atlas_engine_otel::GOAL_RESUMED_METRIC;
use atlas_engine_otel::GOAL_TOKEN_COUNT_METRIC;
use atlas_engine_otel::GOAL_USAGE_LIMITED_METRIC;
use atlas_engine_otel::MetricsClient;

#[derive(Clone, Default)]
pub(crate) struct GoalMetrics {
    metrics_client: Option<MetricsClient>,
}

impl GoalMetrics {
    pub(crate) fn new(metrics_client: Option<MetricsClient>) -> Self {
        Self { metrics_client }
    }

    pub(crate) fn record_created(&self) {
        let Some(metrics_client) = self.metrics_client.as_ref() else {
            return;
        };
        let _ = metrics_client.counter(GOAL_CREATED_METRIC, /*inc*/ 1, &[]);
    }

    pub(crate) fn record_resumed(&self) {
        let Some(metrics_client) = self.metrics_client.as_ref() else {
            return;
        };
        let _ = metrics_client.counter(GOAL_RESUMED_METRIC, /*inc*/ 1, &[]);
    }

    pub(crate) fn record_resumed_if_status_changed(
        &self,
        previous_status: Option<atlas_engine_state::ThreadGoalStatus>,
        goal_status: atlas_engine_state::ThreadGoalStatus,
    ) {
        if goal_status == atlas_engine_state::ThreadGoalStatus::Active
            && matches!(
                previous_status,
                Some(
                    atlas_engine_state::ThreadGoalStatus::Paused
                        | atlas_engine_state::ThreadGoalStatus::Blocked
                        | atlas_engine_state::ThreadGoalStatus::UsageLimited
                )
            )
        {
            self.record_resumed();
        }
    }

    pub(crate) fn record_terminal_if_status_changed(
        &self,
        previous_status: Option<atlas_engine_state::ThreadGoalStatus>,
        goal: &atlas_engine_state::ThreadGoal,
    ) {
        if previous_status == Some(goal.status) {
            return;
        }

        let counter = match goal.status {
            atlas_engine_state::ThreadGoalStatus::Blocked => GOAL_BLOCKED_METRIC,
            atlas_engine_state::ThreadGoalStatus::UsageLimited => GOAL_USAGE_LIMITED_METRIC,
            atlas_engine_state::ThreadGoalStatus::BudgetLimited => GOAL_BUDGET_LIMITED_METRIC,
            atlas_engine_state::ThreadGoalStatus::Complete => GOAL_COMPLETED_METRIC,
            atlas_engine_state::ThreadGoalStatus::Active | atlas_engine_state::ThreadGoalStatus::Paused => {
                return;
            }
        };
        let Some(metrics_client) = self.metrics_client.as_ref() else {
            return;
        };
        let status_tag = [("status", goal.status.as_str())];
        let _ = metrics_client.counter(counter, /*inc*/ 1, &[]);
        let _ = metrics_client.histogram(GOAL_TOKEN_COUNT_METRIC, goal.tokens_used, &status_tag);
        let _ = metrics_client.histogram(
            GOAL_DURATION_SECONDS_METRIC,
            goal.time_used_seconds,
            &status_tag,
        );
    }
}
