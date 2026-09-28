//! Usage limits per account, read only from channels the CLIs themselves
//! expose (ADR Q-0002). See `claude` and `codex`.

pub mod claude;
pub mod codex;

use serde::Serialize;

use crate::accounts::{Account, Provider};

/// One usage window, e.g. the five-hour session or the weekly limit.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// Stable within its account, e.g. `codex-weekly`.
    pub id: String,
    pub label: String,
    /// 0 to 100.
    pub used_percent: f64,
    /// The window's length, when the source says it.
    pub window_minutes: Option<u64>,
    /// Unix seconds.
    pub resets_at: Option<i64>,
}

impl QuotaWindow {
    /// Percent used at `now`: a window past its reset time is empty again.
    pub fn used_at(&self, now: i64) -> f64 {
        match self.resets_at {
            Some(reset) if reset <= now => 0.0,
            _ => self.used_percent,
        }
    }
}

/// Percent left in the tightest window at `now`; `None` without windows.
pub fn headroom(windows: &[QuotaWindow], now: i64) -> Option<f64> {
    windows.iter().map(|window| 100.0 - window.used_at(now)).reduce(f64::min)
}

/// What `account` has left at `now`, or `None` when nothing says. Claude
/// reads the saved status line; Codex asks its app server, which takes a
/// second or two.
pub async fn headroom_of(account: &Account, now: i64) -> Option<f64> {
    match account.provider {
        Provider::Claude => claude::reading_dir(account)
            .and_then(|dir| claude::read(&dir))
            .and_then(|reading| headroom(&reading.windows, now)),
        Provider::Codex => codex::read(account.home.as_deref())
            .await
            .ok()
            .and_then(|reading| headroom(&reading.windows, now)),
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(used: f64, resets_at: Option<i64>) -> QuotaWindow {
        QuotaWindow { id: "w".into(), label: "W".into(), used_percent: used, window_minutes: None, resets_at }
    }

    #[test]
    fn the_tightest_window_decides_and_a_reset_window_is_empty() {
        let windows = [window(30.0, Some(200)), window(90.0, Some(50))];
        assert_eq!(headroom(&windows, 100), Some(70.0));
        assert_eq!(headroom(&windows, 10), Some(10.0));
        assert_eq!(headroom(&[], 10), None);
    }
}
