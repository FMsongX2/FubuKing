//! Tauri adapters over [`crate::quota::QuotaService`]. Thin by design: the
//! service owns the reads, the snapshot and the `atlas:quota-changed` event.

use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::quota::{AccountQuota, QuotaService};

/// The last snapshot, without reading anything. Empty until the first
/// refresh, which starts shortly after launch.
#[tauri::command]
pub fn quota_snapshot(service: State<'_, Arc<QuotaService>>) -> Vec<AccountQuota> {
    service.snapshot()
}

/// Read every account now. Also broadcast, so other windows update too.
#[tauri::command]
pub async fn quota_refresh(app: AppHandle) -> Result<Vec<AccountQuota>, String> {
    let service = app
        .try_state::<Arc<QuotaService>>()
        .ok_or("quota is not ready")?
        .inner()
        .clone();
    Ok(service.refresh(&app).await)
}
