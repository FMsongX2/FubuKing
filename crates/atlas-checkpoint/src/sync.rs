//! Getting the record off the machine and into the Organisation.
//!
//! This is the only module here that touches the network, and everything about
//! its shape follows from one rule: **capture never waits for it**. Offline is
//! the ordinary case, not the error case. Rows accumulate as `pending`, the
//! drain moves them when it can, and nothing upstream ever blocks or reports a
//! failure because a connection was unavailable.
//!
//! # Ordering is blob-first, and that is a rule
//!
//! A row referencing a spilled payload is sent only after that payload is
//! confirmed in blob storage, and is marked `sent` only after the server
//! acknowledges the row. The failure this forbids: a row lands, its blob upload
//! failed, and a *teammate* opens the message to find nothing — a dangling
//! reference in a shared timeline, discovered by the reader rather than the
//! sender. The inverse leak (blob uploaded, row never sent) is tolerable: an
//! unreferenced object costs pennies, not trust, and is logged for a future GC.
//!
//! # Failure is a state machine, not an error
//!
//! * **Offline / 5xx** — rows stay pending, backoff, nothing surfaces.
//! * **429** — honoured with backoff. A rate limit is not a poison row. The
//!   server's `Retry-After` hint, when present, travels out in the outcome so
//!   the host's scheduler can honour it instead of guessing.
//! * **401** — the access token expired. Refresh **once per drain** and
//!   continue *mid-drain*: tokens are short-lived and a post-promotion first
//!   drain runs far longer than one token lifetime. Exactly once, because a
//!   freshly-minted JWT always differs from the previous one — refreshing on
//!   every 401 against a server that keeps saying 401 (clock skew, key
//!   rotation, audience mismatch) is a hot loop, not a retry.
//! * **403** — terminal, on the push path *and* the blob path. Removed from
//!   the Organisation, or the Project was deleted. Retrying forever would be
//!   a permanent spinner; this stops and says "no longer authorized". Local
//!   capture continues untouched.
//! * **A single bad row** — marked failed and skipped, so one malformed record
//!   never stalls everything behind it. Per-artifact results handle this when
//!   the server sends them; when it rejects a whole batch without naming a
//!   row, the drain **bisects**: the batch is halved and retried until a
//!   single-row batch is refused, and that row is the convicted poison. As a
//!   cross-invocation backstop, rows whose attempts exhaust [`MAX_ATTEMPTS`]
//!   are retired to `failed` at the start of every drain.
//! * **A blob missing locally, or permanently too large (413)** — a
//!   permanently unsendable row. Marked failed immediately (the bytes are
//!   gone, or will never fit), never deferred forever under a pending count.

use std::collections::HashSet;
use std::time::Duration;

use crate::artifacts::*;
use crate::error::{Error, Result};
use crate::model::SyncState;
use crate::store::Store;

/// Most artifacts in one request. Matches the server's array cap.
pub const MAX_BATCH_COUNT: usize = 100;

/// Most bytes in one request.
///
/// A count ceiling alone is not enough — a hundred artifacts can be enormous,
/// and the measured corpus contains a single 2.02 MB message. Without this the
/// drain would build a request the server refuses, and retry it forever.
pub const MAX_BATCH_BYTES: usize = 4 * 1024 * 1024;

/// Attempts before a row is treated as poison and skipped.
pub const MAX_ATTEMPTS: i64 = 5;

/// Where the desktop sends artifacts.
///
/// Two rungs — environment variable, then a build-time default — matching the
/// ladder auth already uses. Deliberately **not** telemetry's three-rung ladder
/// with an on-disk override: a file that redirects where a developer's
/// transcripts are sent is a phishing foothold, and one stray write should not
/// be able to create it. An environment variable at least requires code
/// execution in the user's session.
pub fn ingest_base() -> String {
    const DEFAULT: &str = "https://ingest.tryatlas.cc";
    const BUILD: Option<&str> = option_env!("ATLAS_INGEST_URL");

    let raw = std::env::var("ATLAS_INGEST_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| BUILD.map(str::to_string))
        .unwrap_or_else(|| DEFAULT.to_string());
    raw.trim().trim_end_matches('/').to_string()
}

/// Why a drain stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrainStatus {
    /// Everything pending was sent.
    Drained,
    /// The pass ran out of its budget ([`SyncConfig::deadline`]) with rows
    /// still pending. Not a failure and not a reason to back off: the caller
    /// runs the next pass when its turn comes round again, so one Project's
    /// backlog shares the uplink with every other Project's instead of
    /// holding it until the backlog is empty.
    Yielded,
    /// The server could not be reached, or returned 5xx. Rows stay pending and
    /// nothing is surfaced to the developer — this is the ordinary offline case.
    Offline,
    /// Rate limited. Back off and try later.
    RateLimited,
    /// No longer authorized for this Project — removed from the Organisation,
    /// or the Project was deleted server-side. Terminal: retrying is pointless
    /// and presenting it as a transient failure would be a lie. Local capture is
    /// unaffected; only the drain stops.
    NotAuthorized,
    /// There was no credential to send with.
    NoCredential,
}

/// What one drain pass did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainOutcome {
    pub status: DrainStatus,
    pub sent: usize,
    /// Rows the server rejected permanently, marked failed and skipped.
    pub failed: usize,
    pub blobs_uploaded: usize,
    /// Blobs uploaded for a row that then failed permanently. Unreferenced
    /// objects, logged so a future GC can reap them.
    pub orphaned_blobs: usize,
    pub still_pending: i64,
    /// The server's `Retry-After` hint from a 429, when it sent one, so the
    /// host's scheduler can honour it rather than guessing a backoff.
    pub retry_after: Option<Duration>,
}

/// Everything the drain needs from the host.
///
/// The base URL is injected rather than hardcoded so tests drive a real stub
/// server on loopback — the same reason the auth tests can exercise the real
/// client. The token is a closure because access tokens are short-lived and the
/// drain must be able to mint a fresh one *during* a long backlog rather than
/// failing at the first expiry.
pub struct SyncConfig<'a> {
    pub base_url: String,
    pub org_id: String,
    /// Keys the local rows: the path they were written under.
    pub workspace_id: String,
    /// Stamped into every artifact as `workspaceId` — the server-assigned
    /// Project id (or slug fallback), never the local filesystem path, which
    /// no teammate shares and which would leak the directory layout to the
    /// whole Organisation.
    pub wire_workspace_id: String,
    /// Mint or return a current access token. `None` means "not signed in",
    /// which parks the drain rather than failing it.
    pub token: &'a dyn Fn() -> Option<String>,
    pub timeout: Duration,
    /// When the pass stops starting new batches and yields
    /// ([`DrainStatus::Yielded`]). `None` drains until the queue is empty. A
    /// batch already started always finishes: stopping mid-batch would leave
    /// uploaded blobs without their rows.
    pub deadline: Option<std::time::Instant>,
}

impl SyncConfig<'_> {
    fn client(&self) -> Result<reqwest::blocking::Client> {
        reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| Error::Remote(format!("http client: {e}")))
    }
}

/// Send everything pending for a Project.
///
/// Never blocks capture: it runs on its own thread and touches the store only
/// through short transactions. Safe to call repeatedly — the server dedupes on
/// `(seq, contentHash)`, so a replayed batch cannot duplicate rows.
pub fn drain(store: &Store, config: &SyncConfig<'_>) -> Result<DrainOutcome> {
    let mut outcome = DrainOutcome {
        status: DrainStatus::Drained,
        sent: 0,
        failed: 0,
        blobs_uploaded: 0,
        orphaned_blobs: 0,
        still_pending: 0,
        retry_after: None,
    };

    // Cross-invocation backstop for the bisect below: a row whose attempts
    // exhausted the cap leaves the queue before this pass builds its first
    // batch, so convergence survives even a drain that keeps being interrupted
    // before its in-call bisect finishes.
    outcome.failed +=
        store.mark_exhausted_rows_failed(&config.workspace_id, MAX_ATTEMPTS)? as usize;

    // Nothing queued, nothing to send — and no token to mint. The worker ticks
    // every 30 s per Cloud Project, and minting first cost a `GET /token` per
    // tick on an idle Project, which is load the auth server rate-limits.
    if store.row_count_in_state(&config.workspace_id, SyncState::Pending)? == 0 {
        return Ok(outcome);
    }

    let Some(mut token) = (config.token)() else {
        outcome.status = DrainStatus::NoCredential;
        outcome.still_pending = store.row_count_in_state(&config.workspace_id, SyncState::Pending)?;
        return Ok(outcome);
    };

    let client = config.client()?;

    // One token refresh per drain invocation. A freshly-minted JWT always
    // differs from the last (new `iat`), so "refresh whenever the server says
    // 401" spins at full speed the moment the rejection is for any persistent
    // reason. One refresh covers the legitimate case — expiry mid-drain — and
    // a second consecutive 401 stops with `NoCredential`.
    let mut refreshed = false;

    // The bisect ladder. A batch-level rejection names no row, so the batch is
    // halved and retried *within this call* until a single-row batch is
    // refused — that row is the poison, marked failed, and the ladder resets.
    let mut max_count = MAX_BATCH_COUNT;

    loop {
        // Checked between batches, never inside one. At least one batch runs
        // per pass, so a short budget still makes progress.
        if config.deadline.is_some_and(|d| std::time::Instant::now() >= d) && outcome.sent + outcome.failed > 0 {
            outcome.status = DrainStatus::Yielded;
            break;
        }
        let batch = store.pending_artifacts(
            &config.workspace_id,
            &config.wire_workspace_id,
            &config.org_id,
            max_count,
            MAX_BATCH_BYTES,
        )?;
        if batch.is_empty() {
            break;
        }

        // Blob-first. A row is never sent before its payload is confirmed.
        // Rows withheld from this push land in `deferred`; every deferral
        // below also marks the row failed, so the next batch excludes it and
        // the loop converges rather than spinning.
        let mut deferred: HashSet<String> = HashSet::new();
        for artifact in &batch {
            'blobs: for key in artifact.blob_refs() {
                loop {
                    match upload_blob(&client, config, &token, store, key) {
                        Ok(true) => {
                            outcome.blobs_uploaded += 1;
                            break;
                        }
                        Ok(false) => break,
                        Err(BlobError::MissingLocally) => {
                            // The bytes are gone — a deleted `.atlas/blobs`
                            // shard, a database restored without its blobs.
                            // Nothing will ever make this row sendable, so it
                            // is failed now rather than deferred forever under
                            // an eternal pending count.
                            tracing::warn!(
                                row = artifact.row_id(),
                                blob = key,
                                "local blob missing; row marked failed"
                            );
                            store.record_attempt(artifact.row_id())?;
                            store.mark_failed(artifact.row_id())?;
                            outcome.failed += 1;
                            deferred.insert(artifact.row_id().to_string());
                            break 'blobs;
                        }
                        Err(BlobError::TooLarge) => {
                            // 413: this payload will never fit. A permanent
                            // per-row failure, not a network blink.
                            tracing::warn!(
                                row = artifact.row_id(),
                                blob = key,
                                "blob rejected as too large; row marked failed"
                            );
                            store.record_attempt(artifact.row_id())?;
                            store.mark_failed(artifact.row_id())?;
                            outcome.failed += 1;
                            deferred.insert(artifact.row_id().to_string());
                            break 'blobs;
                        }
                        Err(BlobError::Unauthenticated) => {
                            if refresh_once(config, &mut token, &mut refreshed) {
                                // Retry this same blob with the fresh token.
                                continue;
                            }
                            outcome.status = DrainStatus::NoCredential;
                            outcome.still_pending = store
                                .row_count_in_state(&config.workspace_id, SyncState::Pending)?;
                            return Ok(outcome);
                        }
                        Err(BlobError::NotAuthorized) => {
                            // Terminal for the whole drain, exactly as on the
                            // push path — never silently deferred and reported
                            // as drained.
                            outcome.status = DrainStatus::NotAuthorized;
                            outcome.still_pending = store
                                .row_count_in_state(&config.workspace_id, SyncState::Pending)?;
                            return Ok(outcome);
                        }
                        Err(BlobError::RateLimited { retry_after }) => {
                            outcome.status = DrainStatus::RateLimited;
                            outcome.retry_after = retry_after;
                            outcome.still_pending = store
                                .row_count_in_state(&config.workspace_id, SyncState::Pending)?;
                            return Ok(outcome);
                        }
                        Err(BlobError::Offline) => {
                            outcome.status = DrainStatus::Offline;
                            outcome.still_pending = store
                                .row_count_in_state(&config.workspace_id, SyncState::Pending)?;
                            return Ok(outcome);
                        }
                    }
                }
            }
        }
        let ready: Vec<AtlasArtifact> = batch
            .into_iter()
            .filter(|a| !deferred.contains(a.row_id()))
            .collect();
        if ready.is_empty() {
            // Every row in this batch was failed in the blob phase; the next
            // batch excludes them, so continuing makes progress.
            continue;
        }

        match push(&client, config, &token, &ready) {
            Push::Accepted(results) => {
                // An empty result body is the 202 contract: the whole batch
                // was accepted. A *non-empty* body that omits a row is not
                // acceptance — silence is never acknowledgement, so the row
                // stays pending rather than being marked sent on a server
                // that may have dropped it.
                let whole_batch = results.is_empty();
                let mut progressed = false;
                let mut omitted: Vec<&AtlasArtifact> = Vec::new();
                for artifact in &ready {
                    match results.iter().find(|r| r.row_id == artifact.row_id()) {
                        Some(r) if r.accepted => {
                            store.mark_sent(artifact.row_id())?;
                            outcome.sent += 1;
                            progressed = true;
                        }
                        Some(_) => {
                            store.mark_failed(artifact.row_id())?;
                            outcome.failed += 1;
                            outcome.orphaned_blobs += artifact.blob_refs().len();
                            progressed = true;
                        }
                        None if whole_batch => {
                            store.mark_sent(artifact.row_id())?;
                            outcome.sent += 1;
                            progressed = true;
                        }
                        None => omitted.push(artifact),
                    }
                }
                if !progressed {
                    // The server said 2xx but named none of this batch's rows.
                    // Retrying the identical batch in this call would spin;
                    // count an attempt so the exhaustion backstop eventually
                    // retires the rows, and stop here with an honest status.
                    for artifact in &omitted {
                        store.record_attempt(artifact.row_id())?;
                    }
                    outcome.status = DrainStatus::Offline;
                    break;
                }
                max_count = MAX_BATCH_COUNT;
            }
            Push::Unauthenticated => {
                // Short-lived tokens expire mid-drain as a matter of course:
                // a post-promotion backlog runs far longer than one lifetime.
                // Refresh once and retry the same batch; a second consecutive
                // 401 means the credential itself is not accepted.
                if refresh_once(config, &mut token, &mut refreshed) {
                    continue;
                }
                outcome.status = DrainStatus::NoCredential;
                break;
            }
            Push::Forbidden => {
                outcome.status = DrainStatus::NotAuthorized;
                break;
            }
            Push::RateLimited { retry_after } => {
                outcome.status = DrainStatus::RateLimited;
                outcome.retry_after = retry_after;
                break;
            }
            Push::Rejected => {
                // The whole batch was refused without per-artifact detail.
                // Bisect rather than give up: the alternative is failing a
                // hundred good rows because one is malformed.
                if ready.len() == 1 {
                    // The minimal batch was still refused: this row itself is
                    // the poison. A 4xx on a single-row payload is
                    // deterministic — retrying it would only stall the queue
                    // behind it. `retry_failed_rows` is the deliberate,
                    // human-driven road back.
                    let row = ready[0].row_id();
                    tracing::warn!(row, "server rejected a single-row batch; row marked failed");
                    store.record_attempt(row)?;
                    store.mark_failed(row)?;
                    outcome.failed += 1;
                    outcome.orphaned_blobs += ready[0].blob_refs().len();
                    max_count = MAX_BATCH_COUNT;
                } else {
                    // Halve and retry *now*, within this call. Attempts are
                    // counted only against a convicted row — never against the
                    // good rows that happened to share its batch.
                    max_count = (ready.len() / 2).max(1);
                }
            }
            Push::Unreachable => {
                outcome.status = DrainStatus::Offline;
                break;
            }
        }
    }

    outcome.still_pending = store.row_count_in_state(&config.workspace_id, SyncState::Pending)?;
    Ok(outcome)
}

/// Refresh the access token, at most once per drain invocation.
///
/// Returns whether a fresh, *different* token was installed. The once-guard is
/// the point: minting always yields a byte-different JWT, so an unbounded
/// "refresh and retry" against a server that keeps answering 401 is a
/// full-speed hot loop on the capture worker's thread.
fn refresh_once(config: &SyncConfig<'_>, token: &mut String, refreshed: &mut bool) -> bool {
    if *refreshed {
        return false;
    }
    *refreshed = true;
    match (config.token)() {
        Some(fresh) if fresh != *token => {
            *token = fresh;
            true
        }
        _ => false,
    }
}

/// The server's verdict on one push.
enum Push {
    Accepted(Vec<ArtifactResult>),
    /// 401 — the token expired.
    Unauthenticated,
    /// 403 — terminal.
    Forbidden,
    RateLimited { retry_after: Option<Duration> },
    /// 4xx that is not an auth problem: the payload itself was refused.
    Rejected,
    /// No response at all, or 5xx.
    Unreachable,
}

fn push(
    client: &reqwest::blocking::Client,
    config: &SyncConfig<'_>,
    token: &str,
    artifacts: &[AtlasArtifact],
) -> Push {
    let response = client
        .post(format!("{}/ingest", config.base_url))
        .bearer_auth(token)
        .json(&IngestRequest { artifacts: artifacts.to_vec() })
        .send();

    let Ok(response) = response else {
        return Push::Unreachable;
    };

    let status = response.status();
    if status.is_success() {
        // A body with per-artifact results is preferred; its absence means the
        // whole batch was accepted.
        return Push::Accepted(
            response
                .json::<IngestResponse>()
                .map(|r| r.results)
                .unwrap_or_default(),
        );
    }
    match status.as_u16() {
        401 => Push::Unauthenticated,
        403 => Push::Forbidden,
        429 => Push::RateLimited { retry_after: parse_retry_after(&response) },
        // 5xx is the server's problem, not this row's.
        500..=599 => Push::Unreachable,
        _ => Push::Rejected,
    }
}

/// The delta-seconds form of a `Retry-After` header, when present.
///
/// The HTTP-date form is deliberately ignored — our server sends seconds, and
/// a wrong parse would be worse than no hint.
fn parse_retry_after(response: &reqwest::blocking::Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Why one blob upload could not complete.
enum BlobError {
    /// The blob cannot be read locally — the bytes are gone, so the row
    /// referencing it is permanently unsendable.
    MissingLocally,
    /// 401 — the token may have expired; worth one refresh.
    Unauthenticated,
    /// 403 — terminal for the whole drain, same as on the push path.
    NotAuthorized,
    /// 413 — this payload will never fit; a permanent per-row failure.
    TooLarge,
    /// 429 — back off, honouring the server's hint when it sent one.
    RateLimited { retry_after: Option<Duration> },
    /// No response, 5xx, or anything else worth retrying later.
    Offline,
}

/// Upload one blob if the server does not already have it.
///
/// Returns `Ok(true)` when it was uploaded, `Ok(false)` when it was already
/// there, and `Err` when it could not be — with enough shape for the drain to
/// distinguish "this row is dead" from "stop the whole drain".
fn upload_blob(
    client: &reqwest::blocking::Client,
    config: &SyncConfig<'_>,
    token: &str,
    store: &Store,
    key: &str,
) -> std::result::Result<bool, BlobError> {
    let Ok(bytes) = store.blobs().get(key) else {
        return Err(BlobError::MissingLocally);
    };

    let response = client
        .put(format!("{}/blobs/{key}", config.base_url))
        .bearer_auth(token)
        .body(bytes)
        .send();

    let Ok(response) = response else {
        return Err(BlobError::Offline);
    };
    let status = response.status();
    if status.is_success() {
        return Ok(true);
    }
    match status.as_u16() {
        // Content-addressed: already present is success.
        409 => Ok(false),
        401 => Err(BlobError::Unauthenticated),
        403 => Err(BlobError::NotAuthorized),
        413 => Err(BlobError::TooLarge),
        429 => Err(BlobError::RateLimited { retry_after: parse_retry_after(&response) }),
        _ => Err(BlobError::Offline),
    }
}

// ── Project registration (ATL-86) ─────────────────────────────────────────

/// Whether a Slug is free within an Organisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlugAvailability {
    Available,
    Taken,
    /// The check itself could not be made. Distinct from `Taken`, because
    /// telling a developer their name is taken when the network merely blinked
    /// is a lie they will act on.
    Unknown,
}

/// Ask whether `slug` is free.
///
/// Called while typing, debounced by the caller, so the answer arrives before
/// the developer commits to a name rather than as a rejection afterwards.
pub fn check_slug(config: &SyncConfig<'_>, slug: &str) -> SlugAvailability {
    let Some(token) = (config.token)() else {
        return SlugAvailability::Unknown;
    };
    let Ok(client) = config.client() else {
        return SlugAvailability::Unknown;
    };

    let response = client
        .get(format!("{}/workspaces/slug-available", config.base_url))
        .bearer_auth(token)
        .query(&[("orgId", config.org_id.as_str()), ("slug", slug)])
        .send();

    match response {
        Ok(response) if response.status().is_success() => response
            .json::<serde_json::Value>()
            .ok()
            .and_then(|v| v.get("available").and_then(serde_json::Value::as_bool))
            .map(|available| {
                if available {
                    SlugAvailability::Available
                } else {
                    SlugAvailability::Taken
                }
            })
            .unwrap_or_else(|| {
                tracing::warn!(target: "atlas_checkpoint::sync", "slug check: 2xx without an `available` field");
                SlugAvailability::Unknown
            }),
        Ok(response) if response.status() == 409 => SlugAvailability::Taken,
        // `Unknown` is the right answer to the developer, but the reason must
        // land somewhere: the popover renders every refusal as "couldn't check".
        Ok(response) => {
            let err = server_refusal("slug check", response);
            tracing::warn!(target: "atlas_checkpoint::sync", "{err}");
            SlugAvailability::Unknown
        }
        Err(e) => {
            tracing::warn!(target: "atlas_checkpoint::sync", "slug check: {e}");
            SlugAvailability::Unknown
        }
    }
}

/// Who inside the Organisation may read a Project.
///
/// The server's default is `Org`, and so is ours: a Project nobody restricted
/// is one the whole Organisation can read and push to. `Restricted` is the
/// "Restrict to named members" checkbox, and it can be changed later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    #[default]
    Org,
    Restricted,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Org => "org",
            Self::Restricted => "restricted",
        }
    }

    /// Anything unrecognised reads as `Org` rather than failing: visibility is
    /// a display concern here, and the server is the one that enforces it.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "restricted" => Self::Restricted,
            _ => Self::Org,
        }
    }
}

/// What a new Project is called, and who may see it.
///
/// A struct rather than five positional arguments, because four of them are
/// `Option<&str>` and a transposed pair would compile.
#[derive(Debug, Clone, Default)]
pub struct Registration<'a> {
    pub slug: &'a str,
    /// Defaults to the Slug server-side when absent.
    pub name: Option<&'a str>,
    pub root_commit_sha: Option<&'a str>,
    pub git_url: Option<&'a str>,
    pub visibility: Visibility,
}

/// Register a Project with an Organisation.
///
/// **Server first.** The Slug is globally unique within the Organisation, so it
/// has to be settled server-side before anything local changes — otherwise a
/// rejected Slug leaves a half-bound Project behind. The identity signals
/// travel as advisory data: the server must accept a registration with neither.
pub fn register_workspace(config: &SyncConfig<'_>, reg: Registration<'_>) -> Result<String> {
    let token = (config.token)()
        .ok_or_else(|| Error::Remote("not signed in".into()))?;
    let client = config.client()?;
    let slug = reg.slug;

    // Absent fields are **omitted**, never sent as `null`: the server's schema
    // marks `name` optional-but-not-nullable, and a literal `null` there is a
    // `422` for a registration that is otherwise fine. The fingerprints are
    // nullable server-side, but omitting them is right by the same rule.
    let mut body = serde_json::Map::new();
    body.insert("orgId".into(), config.org_id.clone().into());
    body.insert("slug".into(), slug.into());
    body.insert("visibility".into(), reg.visibility.as_str().into());
    insert_present(&mut body, "name", reg.name);
    insert_present(&mut body, "rootCommitSha", reg.root_commit_sha);
    insert_present(&mut body, "gitUrl", reg.git_url);

    let response = client
        .post(format!("{}/workspaces", config.base_url))
        .bearer_auth(token)
        .json(&body)
        .send()
        .map_err(|e| Error::Remote(format!("register project: {e}")))?;

    let status = response.status();
    if status == 409 {
        // Taken between the check and the confirm. Told plainly, and nothing
        // local has changed.
        return Err(Error::Remote(format!("the slug \"{slug}\" is already taken")));
    }
    if !status.is_success() {
        return Err(server_refusal("register project", response));
    }

    response
        .json::<serde_json::Value>()
        .ok()
        .and_then(|v| {
            v.get("workspaceId")
                .or_else(|| v.get("id"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| Error::Remote("register project: no id in response".into()))
}

/// A Project as the Organisation knows it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorkspace {
    pub id: String,
    pub slug: String,
    /// Advisory fingerprints, as whoever created the Project supplied them.
    #[serde(default)]
    pub root_commit_sha: Option<String>,
    #[serde(default)]
    pub git_url: Option<String>,
    /// Display name. Defaulted because a Project registered before the server
    /// carried one answers without the field.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub visibility: Visibility,
}

/// Which Project the popover should pre-select, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preselection {
    /// Exactly one Project matches. One click.
    One { project: RemoteWorkspace, reason: MatchReason },
    /// Several matched. **Pre-select nothing** and show them all.
    ///
    /// Every repository created from the same GitHub template shares a root
    /// commit, so a fingerprint match is *not* proof of the same project. A
    /// confident wrong pre-selection is worse than none: connecting the wrong
    /// directory pollutes a **shared** Organisation timeline with foreign
    /// Sessions, and nobody notices until someone reads them.
    Ambiguous { candidates: Vec<RemoteWorkspace> },
    /// Nothing matched. Warn, then trust the human — a shallow clone, a squashed
    /// history, a fresh repository joining existing work and a monorepo
    /// subdirectory are all legitimate, and a hard block would break every one.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchReason {
    /// Same root commit. Survives forks, folder renames and remote changes.
    Fingerprint,
    /// Same normalised origin, where the fingerprint did not match.
    OriginUrl,
}

/// Decide what to pre-select for a local repository.
///
/// Pure, so the rule that actually matters — ambiguity pre-selects nothing — is
/// testable without a server.
pub fn preselect(
    projects: &[RemoteWorkspace],
    root_commit_sha: Option<&str>,
    git_url: Option<&str>,
) -> Preselection {
    // The fingerprint is tried first because it is the identity that survives a
    // fork: same root commit, different origin URL.
    if let Some(sha) = root_commit_sha.filter(|s| !s.is_empty()) {
        let matches: Vec<RemoteWorkspace> = projects
            .iter()
            .filter(|w| w.root_commit_sha.as_deref() == Some(sha))
            .cloned()
            .collect();
        match matches.len() {
            1 => {
                return Preselection::One {
                    project: matches[0].clone(),
                    reason: MatchReason::Fingerprint,
                }
            }
            0 => {}
            _ => return Preselection::Ambiguous { candidates: matches },
        }
    }

    if let Some(url) = git_url.filter(|u| !u.is_empty()) {
        let matches: Vec<RemoteWorkspace> = projects
            .iter()
            .filter(|w| w.git_url.as_deref() == Some(url))
            .cloned()
            .collect();
        match matches.len() {
            1 => {
                return Preselection::One {
                    project: matches[0].clone(),
                    reason: MatchReason::OriginUrl,
                }
            }
            0 => {}
            _ => return Preselection::Ambiguous { candidates: matches },
        }
    }

    Preselection::None
}

/// Every Project in the Organisation the developer can reach.
pub fn list_workspaces(config: &SyncConfig<'_>) -> Result<Vec<RemoteWorkspace>> {
    let token = (config.token)().ok_or_else(|| Error::Remote("not signed in".into()))?;
    let client = config.client()?;

    let response = client
        .get(format!("{}/workspaces", config.base_url))
        .bearer_auth(token)
        .query(&[("orgId", config.org_id.as_str())])
        .send()
        .map_err(|e| Error::Remote(format!("list workspaces: {e}")))?;

    if !response.status().is_success() {
        let err = server_refusal("list workspaces", response);
        tracing::warn!(target: "atlas_checkpoint::sync", "{err}");
        return Err(err);
    }
    let body: serde_json::Value = response
        .json()
        .map_err(|e| Error::Remote(format!("list workspaces: unreadable response: {e}")))?;
    let rows = body
        .get("workspaces")
        .cloned()
        .ok_or_else(|| Error::Remote("list workspaces: response carries no `workspaces`".into()))?;
    serde_json::from_value(rows).map_err(|e| {
        // A row this build cannot read is worth naming: the alternative was a
        // bare "unreadable response" that looked like a network fault.
        let err = Error::Remote(format!("list workspaces: a workspace row did not parse: {e}"));
        tracing::warn!(target: "atlas_checkpoint::sync", "{err}");
        err
    })
}

/// What the developer is asking to connect to.
///
/// `workspace_id` is an explicit pick from the picker. With neither it nor
/// `slug` set, the server matches on the fingerprints alone — which is how a
/// teammate's fresh checkout finds the Project without being told its name.
#[derive(Debug, Clone, Default)]
pub struct ConnectRequest<'a> {
    pub workspace_id: Option<&'a str>,
    pub slug: Option<&'a str>,
    pub root_commit_sha: Option<&'a str>,
    pub git_url: Option<&'a str>,
    /// Create the Project when nothing matched. Requires `slug`.
    pub create: bool,
}

/// How the server resolved a connect request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    Connected {
        workspace_id: String,
        slug: Option<String>,
        /// `explicit`, `fingerprint`, `origin_url`, `created` or
        /// `already_connected` — worth showing, because "we matched your root
        /// commit" and "you picked this" earn different amounts of trust.
        reason: Option<String>,
    },
    /// Several Projects matched and the server bound none of them.
    ///
    /// Not an error: every repository made from the same template shares a root
    /// commit, so this is the *correct* answer and the human has to pick.
    Ambiguous { candidates: Vec<RemoteWorkspace> },
    /// Nothing matched, and `create` was not set.
    NoMatch,
}

/// Connect this repository to a Project, letting the server do the matching.
///
/// Preferred over registering blind: the server checks the root commit first
/// and only then the origin URL, refuses to guess when several match, and opens
/// the sync run in the same call. Idempotent — connecting twice resumes the
/// existing run rather than starting a second one.
pub fn connect_workspace(
    config: &SyncConfig<'_>,
    req: ConnectRequest<'_>,
) -> Result<ConnectOutcome> {
    let token = (config.token)().ok_or_else(|| Error::Remote("not signed in".into()))?;
    let client = config.client()?;

    // Same rule as `register_workspace`: `workspaceId` and `slug` are optional
    // server-side but not nullable, and the common path — match on the
    // fingerprints alone — sets neither.
    let mut body = serde_json::Map::new();
    body.insert("orgId".into(), config.org_id.clone().into());
    body.insert("create".into(), req.create.into());
    insert_present(&mut body, "workspaceId", req.workspace_id);
    insert_present(&mut body, "slug", req.slug);
    insert_present(&mut body, "rootCommitSha", req.root_commit_sha);
    insert_present(&mut body, "gitUrl", req.git_url);

    let response = client
        .post(format!("{}/workspaces/connect", config.base_url))
        .bearer_auth(token)
        .json(&body)
        .send()
        .map_err(|e| Error::Remote(format!("connect project: {e}")))?;

    let status = response.status();
    if status == 409 {
        let slug = req.slug.unwrap_or("that slug");
        return Err(Error::Remote(format!("the slug \"{slug}\" is already taken")));
    }
    if !status.is_success() {
        return Err(server_refusal("connect project", response));
    }

    let body: serde_json::Value = response
        .json()
        .map_err(|e| Error::Remote(format!("connect project: unreadable response: {e}")))?;

    Ok(read_connect_outcome(&body))
}

/// Put `value` on the body only when there is one.
///
/// `serde_json::json!` writes an `Option::None` as `null`, and the registry's
/// schemas accept an *absent* optional but refuse a `null` one.
fn insert_present(body: &mut serde_json::Map<String, serde_json::Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        body.insert(key.to_string(), value.into());
    }
}

/// Turn a refused response into an error that says what the server said.
///
/// The registry answers `{"error":{"code","message"}}` and the message is the
/// first schema issue — "Expected string, received null" — which is the one
/// fact a person debugging a `422` needs and the status alone withholds.
fn server_refusal(what: &str, response: reqwest::blocking::Response) -> Error {
    let status = response.status();
    let reason = response
        .json::<serde_json::Value>()
        .ok()
        .and_then(|v| server_error_message(&v));
    match reason {
        Some(reason) => Error::Remote(format!("{what}: server returned {status} ({reason})")),
        None => Error::Remote(format!("{what}: server returned {status}")),
    }
}

/// The `error.message` (and `error.code` when it adds something) of a
/// registry refusal, or `None` when the body is not that shape.
fn server_error_message(body: &serde_json::Value) -> Option<String> {
    let error = body.get("error")?;
    let message = error.get("message").and_then(serde_json::Value::as_str);
    let code = error.get("code").and_then(serde_json::Value::as_str);
    match (code, message) {
        (Some(code), Some(message)) if message != code => Some(format!("{code}: {message}")),
        (_, Some(message)) => Some(message.to_string()),
        (Some(code), None) => Some(code.to_string()),
        (None, None) => None,
    }
}

/// Split out from the request so the response shapes are testable without a
/// server — including the one that matters, `ambiguous` binding nothing.
fn read_connect_outcome(body: &serde_json::Value) -> ConnectOutcome {
    let str_at = |key: &str| {
        body.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };

    match body.get("status").and_then(serde_json::Value::as_str) {
        Some("connected") => match str_at("workspaceId") {
            Some(workspace_id) => ConnectOutcome::Connected {
                workspace_id,
                slug: str_at("slug"),
                reason: str_at("reason"),
            },
            // `connected` without an id is not something to act on.
            None => ConnectOutcome::NoMatch,
        },
        Some("ambiguous") => ConnectOutcome::Ambiguous {
            candidates: body
                .get("candidates")
                .cloned()
                .and_then(|c| serde_json::from_value(c).ok())
                .unwrap_or_default(),
        },
        _ => ConnectOutcome::NoMatch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: &str, sha: Option<&str>, url: Option<&str>) -> RemoteWorkspace {
        RemoteWorkspace {
            id: id.to_string(),
            slug: id.to_string(),
            root_commit_sha: sha.map(str::to_string),
            git_url: url.map(str::to_string),
            name: None,
            visibility: Visibility::Org,
        }
    }

    #[test]
    fn a_connected_response_carries_the_id_the_server_chose() {
        // Not the id we asked for: the server may resolve a slug, or answer
        // `already_connected` with the Project we were bound to all along.
        let body = serde_json::json!({
            "status": "connected",
            "workspaceId": "ws-remote-9",
            "slug": "atlas",
            "reason": "fingerprint",
        });
        assert_eq!(
            read_connect_outcome(&body),
            ConnectOutcome::Connected {
                workspace_id: "ws-remote-9".into(),
                slug: Some("atlas".into()),
                reason: Some("fingerprint".into()),
            }
        );
    }

    #[test]
    fn an_ambiguous_response_binds_nothing_and_keeps_the_candidates() {
        // The rule that matters. Two repositories from one template share a
        // root commit, and picking either would pollute a shared timeline.
        let body = serde_json::json!({
            "status": "ambiguous",
            "workspaceId": serde_json::Value::Null,
            "candidates": [
                { "id": "a", "slug": "a", "rootCommitSha": "root-1" },
                { "id": "b", "slug": "b", "rootCommitSha": "root-1" },
            ],
        });
        let ConnectOutcome::Ambiguous { candidates } = read_connect_outcome(&body) else {
            panic!("ambiguous must not bind");
        };
        assert_eq!(candidates.len(), 2);
        // The fields the server omits must default rather than drop the row.
        assert_eq!(candidates[0].visibility, Visibility::Org);
        assert_eq!(candidates[0].name, None);
    }

    #[test]
    fn connected_without_an_id_is_not_acted_on() {
        // A malformed success is not a binding. Treating it as one would write
        // `None` as the wire identity and fail at drain time instead of here.
        let body = serde_json::json!({ "status": "connected" });
        assert_eq!(read_connect_outcome(&body), ConnectOutcome::NoMatch);
    }

    #[test]
    fn an_unrecognised_status_is_no_match() {
        assert_eq!(
            read_connect_outcome(&serde_json::json!({ "status": "no_match" })),
            ConnectOutcome::NoMatch
        );
        assert_eq!(read_connect_outcome(&serde_json::json!({})), ConnectOutcome::NoMatch);
    }

    #[test]
    fn visibility_round_trips_and_unknown_reads_as_org() {
        assert_eq!(Visibility::parse("restricted"), Visibility::Restricted);
        assert_eq!(Visibility::Restricted.as_str(), "restricted");
        assert_eq!(Visibility::parse("org"), Visibility::Org);
        // An unrecognised value must not fail the picker; the server enforces.
        assert_eq!(Visibility::parse("public"), Visibility::Org);
        assert_eq!(Visibility::default(), Visibility::Org);
    }

    #[test]
    fn a_single_fingerprint_match_preselects_it() {
        let all = vec![
            project("atlas", Some("root-1"), Some("github.com/tryatlas/atlas")),
            project("server", Some("root-2"), Some("github.com/tryatlas/server")),
        ];
        assert!(matches!(
            preselect(&all, Some("root-1"), None),
            Preselection::One { reason: MatchReason::Fingerprint, .. }
        ));
    }

    #[test]
    fn a_fork_preselects_correctly_despite_a_different_origin() {
        // Same root commit, different remote — the case the fingerprint exists
        // for.
        let all = vec![project("atlas", Some("root-1"), Some("github.com/tryatlas/atlas"))];
        let chosen = preselect(&all, Some("root-1"), Some("github.com/nafiz/atlas-fork"));
        assert!(matches!(
            chosen,
            Preselection::One { reason: MatchReason::Fingerprint, .. }
        ));
    }

    #[test]
    fn an_origin_match_preselects_when_the_fingerprint_does_not() {
        // A squashed history has a different root commit but the same remote.
        let all = vec![project("atlas", Some("other-root"), Some("github.com/tryatlas/atlas"))];
        assert!(matches!(
            preselect(&all, Some("root-1"), Some("github.com/tryatlas/atlas")),
            Preselection::One { reason: MatchReason::OriginUrl, .. }
        ));
    }

    #[test]
    fn several_fingerprint_matches_preselect_nothing() {
        // Every repository made from the same GitHub template shares a root
        // commit, so a match is not proof. A confident wrong answer here
        // pollutes a shared timeline with foreign Sessions.
        let all = vec![
            project("from-template-a", Some("template-root"), None),
            project("from-template-b", Some("template-root"), None),
        ];
        match preselect(&all, Some("template-root"), None) {
            Preselection::Ambiguous { candidates } => {
                assert_eq!(candidates.len(), 2, "all matches are shown");
            }
            other => panic!("expected no pre-selection, got {other:?}"),
        }
    }

    #[test]
    fn nothing_matching_preselects_nothing_but_does_not_block() {
        assert_eq!(
            preselect(&[project("atlas", Some("root-1"), None)], Some("unrelated"), None),
            Preselection::None
        );
    }

    #[test]
    fn a_repository_with_no_signals_at_all_preselects_nothing() {
        // A shallow clone with no remote, or a directory that is not a
        // repository. Still connectable — the developer chooses.
        assert_eq!(
            preselect(&[project("atlas", Some("root-1"), None)], None, None),
            Preselection::None
        );
    }

    /// One test, because these mutate a process-global and `cargo test` runs
    /// tests in parallel — split apart they race each other and fail at random.
    #[test]
    fn the_ingest_base_resolves_through_its_ladder() {
        std::env::set_var("ATLAS_INGEST_URL", "http://127.0.0.1:9999/");
        assert_eq!(
            ingest_base(),
            "http://127.0.0.1:9999",
            "an override wins, with no trailing slash"
        );

        std::env::set_var("ATLAS_INGEST_URL", "   ");
        assert!(
            ingest_base().starts_with("https://"),
            "an empty override falls through rather than pointing at nothing"
        );

        std::env::remove_var("ATLAS_INGEST_URL");
    }
}
