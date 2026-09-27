//! The remote half of the board, held in memory.
//!
//! # Why a cache rather than a read-through
//!
//! `artifacts_board` is called on every capture event and every git change
//! while the Timeline is open. If it awaited the network, the board would stall
//! behind a request on a surface whose whole appeal is that it is instant, and
//! it would show nothing at all offline — where the local Sessions are still
//! perfectly readable.
//!
//! So the network read and the board read are separated. A refresher fills this
//! cache in the background and the socket patches it; the board read takes a
//! lock and merges. A cold cache means a board with only local rows, which is
//! exactly the right offline answer.
//!
//! # Why it is keyed by Organisation, not by Project
//!
//! Every board row carries its own `workspaceId`, and `GET /sessions?org=` fans
//! out across the Organisation in one call. Keying per Project would mean one
//! request per Project to assemble the same list, and would leave Projects this
//! machine has never bound invisible — which is most of them, for anyone who
//! has not checked out every repository the team owns.
//!
//! # Why it is not persisted
//!
//! A remote Session is someone else's record of work that did not happen here.
//! Keeping a stale copy across restarts would put rows on the board that may
//! since have been deleted, with no way to notice.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::model::{RemoteProject, RemoteSession};

/// Which Project, in which Organisation — the socket's unit, not the board's.
///
/// Both are **server** ids: a local project path means nothing to the
/// Organisation, and two checkouts of one repository share a Project.
pub type ProjectKey = (String, String);

/// One Organisation's remote board, as of the last refresh.
#[derive(Debug, Clone, Default)]
pub struct OrgBoard {
    /// Keyed by Session id, which is the same id the local store minted — so
    /// merging with the local board is a keyed union, not a reconciliation.
    pub sessions: HashMap<String, RemoteSession>,
    /// Project id → how to name it, for a row from a Project this machine has
    /// no checkout of.
    pub projects: HashMap<String, RemoteProject>,
    /// Server-side caveats from the last refresh — an unreachable Project, or
    /// the fan-out cap. Surfaced rather than swallowed: a board quietly missing
    /// a Project looks exactly like a Project with no work in it.
    pub notes: Vec<String>,
    /// Has a refresh ever succeeded? A board that has never loaded and one that
    /// is genuinely empty look identical otherwise.
    pub loaded: bool,
    /// Has a refresh ever *finished*, successfully or not?
    ///
    /// Distinct from `loaded` because the viewer needs "still waiting" to end
    /// even when the answer never arrives. Gating a loading state on `loaded`
    /// alone leaves an Organisation that cannot reach the server showing a
    /// skeleton for ever; gating it on this shows the honest empty board.
    pub attempted: bool,
}

#[derive(Default)]
pub struct CloudBoard {
    orgs: RwLock<HashMap<String, OrgBoard>>,
}

impl CloudBoard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace an Organisation's rows wholesale.
    ///
    /// Wholesale rather than merged, so a Session deleted server-side actually
    /// leaves the board — nothing ever announces a deletion, so a merge would
    /// keep it forever. The socket's incremental updates go through
    /// [`Self::upsert`] instead.
    pub fn replace(
        &self,
        org_id: &str,
        sessions: Vec<RemoteSession>,
        projects: Vec<RemoteProject>,
        notes: Vec<String>,
    ) {
        let board = OrgBoard {
            sessions: sessions.into_iter().map(|s| (s.id.clone(), s)).collect(),
            projects: projects.into_iter().map(|p| (p.id.clone(), p)).collect(),
            notes,
            loaded: true,
            attempted: true,
        };
        if let Ok(mut orgs) = self.orgs.write() {
            orgs.insert(org_id.to_string(), board);
        }
    }

    /// Apply one `session.summary` frame.
    ///
    /// Lands even on an Organisation that has never refreshed: the frame is a
    /// complete row, and dropping it because the first page had not arrived
    /// would lose the live Session it is usually announcing. `loaded` stays
    /// false so a refresh still runs.
    pub fn upsert(&self, org_id: &str, session: RemoteSession) {
        if let Ok(mut orgs) = self.orgs.write() {
            orgs.entry(org_id.to_string())
                .or_default()
                .sessions
                .insert(session.id.clone(), session);
        }
    }

    /// Everything known about one Organisation. Synchronous and lock-only —
    /// this is what the board read calls.
    pub fn snapshot(&self, org_id: &str) -> OrgBoard {
        self.orgs
            .read()
            .ok()
            .and_then(|orgs| orgs.get(org_id).cloned())
            .unwrap_or_default()
    }

    /// Record that a refresh finished without changing any rows.
    ///
    /// Called on the failure path, so a viewer waiting on the first answer
    /// stops waiting. Deliberately does **not** clear the rows a previous
    /// refresh found: one blocked request says nothing about work that is
    /// genuinely there.
    pub fn mark_attempted(&self, org_id: &str) {
        if let Ok(mut orgs) = self.orgs.write() {
            orgs.entry(org_id.to_string()).or_default().attempted = true;
        }
    }

    /// Is the first refresh for this Organisation still outstanding?
    ///
    /// What separates "this Organisation has no Sessions" from "we have not
    /// looked yet" — the board shows a skeleton for the second and an empty
    /// state for the first, and they used to be indistinguishable.
    pub fn is_pending(&self, org_id: &str) -> bool {
        self.orgs
            .read()
            .ok()
            .and_then(|orgs| orgs.get(org_id).map(|board| board.attempted))
            .is_none_or(|attempted| !attempted)
    }

    /// Has every refresh so far failed?
    ///
    /// A third state beyond pending and ready: the board is done waiting but is
    /// showing a local-only view of an Organisation that has more in it. The
    /// viewer needs to say so and offer a retry, rather than present a partial
    /// board as the whole truth. Goes false the moment any refresh succeeds.
    pub fn has_failed(&self, org_id: &str) -> bool {
        self.orgs
            .read()
            .ok()
            .and_then(|orgs| orgs.get(org_id).map(|board| board.attempted && !board.loaded))
            .unwrap_or(false)
    }

    /// Drop everything. Called on an Organisation switch, so the incoming
    /// tenant inherits nothing — not even for a frame.
    pub fn clear(&self) {
        if let Ok(mut orgs) = self.orgs.write() {
            orgs.clear();
        }
    }

    /// Drop one Project's rows, for a disconnect or a membership revocation.
    ///
    /// Per-Project rather than per-Organisation: losing access to one Project
    /// says nothing about the rest of the Organisation.
    pub fn forget_project(&self, key: &ProjectKey) {
        let (org_id, project_id) = key;
        if let Ok(mut orgs) = self.orgs.write() {
            if let Some(board) = orgs.get_mut(org_id) {
                board.sessions.retain(|_, s| &s.workspace_id != project_id);
                board.projects.remove(project_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, project: &str, activity: &str) -> RemoteSession {
        RemoteSession {
            id: id.into(),
            workspace_id: project.into(),
            last_activity_at: activity.into(),
            ..RemoteSession::default()
        }
    }

    #[test]
    fn a_refresh_replaces_rather_than_merges() {
        // A Session deleted server-side has to leave the board. Merging would
        // keep it forever, because nothing ever announces a deletion.
        let board = CloudBoard::new();
        board.replace("org_1", vec![session("a", "ws_1", "t1"), session("b", "ws_1", "t1")], vec![], vec![]);
        board.replace("org_1", vec![session("b", "ws_1", "t2")], vec![], vec![]);

        let snap = board.snapshot("org_1");
        assert_eq!(snap.sessions.len(), 1);
        assert!(snap.sessions.contains_key("b"));
        assert!(snap.loaded);
    }

    #[test]
    fn a_frame_for_an_unrefreshed_org_is_kept() {
        // The common case for a teammate starting work: the summary frame
        // arrives before this desktop has listed anything.
        let board = CloudBoard::new();
        board.upsert("org_1", session("live", "ws_1", "t1"));

        let snap = board.snapshot("org_1");
        assert_eq!(snap.sessions.len(), 1);
        // Still unloaded, so a refresh runs and fills in the rest.
        assert!(!snap.loaded);
    }

    #[test]
    fn a_frame_updates_the_row_in_place_rather_than_duplicating_it() {
        let board = CloudBoard::new();
        board.replace("org_1", vec![session("a", "ws_1", "t1")], vec![], vec![]);
        board.upsert("org_1", session("a", "ws_1", "t2"));

        let snap = board.snapshot("org_1");
        assert_eq!(snap.sessions.len(), 1);
        assert_eq!(snap.sessions["a"].last_activity_at, "t2");
    }

    #[test]
    fn an_unknown_org_reads_as_empty_rather_than_failing() {
        // The offline answer, and the answer before the first refresh lands.
        let snap = CloudBoard::new().snapshot("org_nope");
        assert!(snap.sessions.is_empty());
        assert!(snap.notes.is_empty());
        assert!(!snap.loaded);
    }

    #[test]
    fn an_organisation_is_pending_until_a_refresh_finishes() {
        // The distinction the board's loading state hangs on: "no Sessions" and
        // "not looked yet" are different answers and used to look the same.
        let board = CloudBoard::new();
        assert!(board.is_pending("org_1"));

        board.replace("org_1", vec![], vec![], vec![]);
        assert!(!board.is_pending("org_1"));
    }

    #[test]
    fn a_failed_refresh_still_ends_the_wait() {
        // Otherwise an Organisation that cannot reach the server shows a
        // skeleton for ever instead of an honest empty board.
        let board = CloudBoard::new();
        board.mark_attempted("org_1");
        assert!(!board.is_pending("org_1"));
        assert!(!board.snapshot("org_1").loaded);
    }

    #[test]
    fn a_failed_refresh_does_not_discard_rows_a_good_one_found() {
        // One blocked request says nothing about work that is genuinely there.
        let board = CloudBoard::new();
        board.replace("org_1", vec![session("a", "ws_1", "t1")], vec![], vec![]);
        board.mark_attempted("org_1");
        assert_eq!(board.snapshot("org_1").sessions.len(), 1);
    }

    #[test]
    fn a_frame_before_any_refresh_leaves_the_org_pending() {
        // A summary frame is one row, not the board — the refresh is still owed.
        let board = CloudBoard::new();
        board.upsert("org_1", session("live", "ws_1", "t1"));
        assert!(board.is_pending("org_1"));
    }

    #[test]
    fn a_failed_refresh_is_distinguishable_from_an_empty_one() {
        // Three states, not two: never looked, looked and found nothing, and
        // could not look. The middle and the last render differently.
        let board = CloudBoard::new();
        assert!(!board.has_failed("org_1"), "never looked is not failed");

        board.mark_attempted("org_1");
        assert!(board.has_failed("org_1"));

        // Any success clears it, even one that finds nothing.
        board.replace("org_1", vec![], vec![], vec![]);
        assert!(!board.has_failed("org_1"));
    }

    #[test]
    fn clearing_leaves_nothing_for_the_next_organisation() {
        let board = CloudBoard::new();
        board.replace("org_1", vec![session("a", "ws_1", "t1")], vec![], vec!["a note".into()]);
        board.clear();
        assert!(board.snapshot("org_1").sessions.is_empty());
    }

    #[test]
    fn forgetting_one_project_leaves_the_others_in_the_same_org() {
        // Losing access to one Project says nothing about the rest.
        let board = CloudBoard::new();
        board.replace(
            "org_1",
            vec![session("a", "ws_1", "t1"), session("b", "ws_2", "t1")],
            vec![
                RemoteProject { id: "ws_1".into(), slug: None, name: None },
                RemoteProject { id: "ws_2".into(), slug: None, name: None },
            ],
            vec![],
        );

        board.forget_project(&("org_1".into(), "ws_1".into()));
        let snap = board.snapshot("org_1");
        assert_eq!(snap.sessions.len(), 1);
        assert!(snap.sessions.contains_key("b"));
        assert!(!snap.projects.contains_key("ws_1"));
        assert!(snap.projects.contains_key("ws_2"));
    }
}
