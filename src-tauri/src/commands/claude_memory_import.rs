// Modified by Quotatlas from upstream Atlas (Apache-2.0).
//! Consented import of Claude's auto-memory into shared memory.
//!
//! Claude Code keeps its own per-project memory: one markdown file per memory
//! under `~/.claude/projects/<encoded dir>/memory/`, each with YAML
//! frontmatter (`name`, `description`, and a `type` of `user`, `feedback`,
//! `project` or `reference` — top level, or nested under `metadata:`). Atlas
//! keeps *reading* those files continuously (`agent_memory::read_claude`);
//! this module is the separate, user-triggered step that copies them into the
//! record so every agent on the repository gets them.
//!
//! The flow is two commands and nothing else:
//!
//! - **Preview** (`memory_claude_import_preview`) reads the files and returns
//!   every mapped line with its kind and whether it would be new. It writes
//!   nothing — cancelling the dialog is simply never calling confirm.
//! - **Confirm** (`memory_claude_import_confirm`) re-reads the files and writes
//!   the new lines the user kept (by preview id) with confidence 0.7 and
//!   source `import:claude`, then records the import of each source.
//!
//! **Mapping.** One memory file is one line: its `description` when it has
//! one (Claude writes it as the one-line summary), else the first paragraph
//! of its body. `feedback`, `reference` and `user` map to Fact; `project`
//! maps to Decision when the text [states a choice](states_a_choice), else
//! Fact. `MEMORY.md` is Claude's index of the other files and is skipped.
//! Every file is read through `strip_injected_context`, so an `<atlas-memory>`
//! envelope Claude saved back never re-enters the record; a line that still
//! carries the envelope tag is dropped.
//!
//! **Which Claude directories belong to a scope.** Claude's docs derive the
//! auto-memory project from the git repository (shared across worktrees), but
//! older versions keyed it by the launch directory, so one repository can
//! have several: the scope root (main worktree), every linked worktree git
//! knows of, and the launch directory itself (a subdirectory launch). The
//! same set the record store migrates legacy memory from (`store_for`); each
//! existing `memory/` directory among them is one import source.
//!
//! **Once per source.** Confirm records `claude-memory:<memory dir>` in the
//! record's `legacy_imports` table (the gate the legacy migration uses); a
//! recorded source contributes no new lines again, so a second import is a
//! no-op and a memory the user forgot after importing is never re-offered.
//! Independently of the gate, a line whose kind and content hash the record
//! already holds is not new either, so re-running shows nothing new even
//! without the gate row.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use atlas_memory::record::{self, EntryKind, NewEntry, WriteOutcome};
use serde::Serialize;
use tauri::State;

use super::agent_memory::{claude_memory_dir, parse_frontmatter, read_without_injected_context};
use super::shared_memory::{store_for, SharedMemoryStore};

/// Provenance of every imported entry.
pub(crate) const CLAUDE_IMPORT_SOURCE: &str = "import:claude";

/// Confidence of every imported entry: another program's memory, kept by the
/// user's consent but not confirmed by them line by line.
pub(crate) const CLAUDE_IMPORT_CONFIDENCE: f64 = 0.7;

/// The envelope tag Atlas wraps injected context in. Stripped on read; a line
/// that still carries it (a malformed envelope) is not imported.
const ENVELOPE_TAG: &str = "<atlas-memory>";

/// One mapped line, as the preview lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLine {
    /// Stable id of this line (kind + content hash); what confirm takes.
    pub id: String,
    /// `fact` or `decision`.
    pub kind: EntryKind,
    /// The text that would be written (redacted).
    pub content: String,
    /// The Claude memory file it came from (file name).
    pub file: String,
    /// Claude's own `type` for it (`user`, `feedback`, `project`, `reference`;
    /// empty when the file has none).
    pub claude_type: String,
    /// `false` when its source was already imported or the record already
    /// holds it: confirm skips it.
    pub is_new: bool,
}

/// What an import would do.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeImportPreview {
    /// The Claude memory directories read for this scope.
    pub sources: Vec<String>,
    /// Every source found has been imported before.
    pub already_imported: bool,
    pub lines: Vec<ImportLine>,
}

// ── Mapping ──────────────────────────────────────────────────────────────────

/// Whether `text` states a choice — the test that makes a `project` memory a
/// Decision rather than a Fact.
///
/// A deliberately simple heuristic: the text uses a word of deciding
/// (`decided`, `decision`, `chose`, `chosen`, `opted`, `prefer`,
/// `prefers`, `preferred`, `picked`, `adopted`, `locked`) or a phrase that
/// sets one option against another or commits to one (`instead of`,
/// `rather than`, `in favor of`, `in favour of`, `will use`, `switched to`,
/// `went with`, `settled on`, `standardized on`, `standardised on`).
/// Case-insensitive, whole words only.
pub fn states_a_choice(text: &str) -> bool {
    const WORDS: &[&str] = &[
        "decided", "decision", "decisions", "chose", "chosen", "opted", "prefer", "prefers",
        "preferred", "picked", "adopted", "locked",
    ];
    const PHRASES: &[&str] = &[
        "instead of",
        "rather than",
        "in favor of",
        "in favour of",
        "will use",
        "switched to",
        "went with",
        "settled on",
        "standardized on",
        "standardised on",
    ];
    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    if words.iter().any(|w| WORDS.contains(&w.as_str())) {
        return true;
    }
    let joined = format!(" {} ", words.join(" "));
    PHRASES.iter().any(|p| joined.contains(&format!(" {p} ")))
}

/// Claude's frontmatter `type` (with the memory's text) → the record kind.
pub fn map_kind(claude_type: &str, text: &str) -> EntryKind {
    match claude_type.trim().to_ascii_lowercase().as_str() {
        "project" if states_a_choice(text) => EntryKind::Decision,
        _ => EntryKind::Fact,
    }
}

/// The one line a memory file becomes: its description, else the first
/// paragraph of its body (headings skipped), collapsed to one line.
fn line_of(description: &str, body: &str) -> String {
    let description = description.trim();
    if !description.is_empty() {
        return collapse(description);
    }
    let mut paragraph: Vec<&str> = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() {
            if paragraph.is_empty() {
                continue;
            }
            break;
        }
        if paragraph.is_empty() && t.starts_with('#') {
            continue;
        }
        paragraph.push(t);
    }
    collapse(&paragraph.join(" "))
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn line_id(kind: EntryKind, content: &str) -> String {
    let hash = record::content_hash(content);
    format!("{}:{}", kind.as_str(), &hash[..16])
}

/// A line read from one source, before the record is consulted.
struct Mapped {
    kind: EntryKind,
    content: String,
    file: String,
    claude_type: String,
}

/// Every memory file in `dir` mapped to its line, in file-name order.
fn read_dir(dir: &Path) -> Vec<Mapped> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = read
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter(|p| p.file_name().is_some_and(|n| n != "MEMORY.md"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let Some(raw) = read_without_injected_context(&path) else {
            continue;
        };
        let (meta, body) = parse_frontmatter(&raw);
        let content = record::redact(&line_of(meta.description.as_deref().unwrap_or(""), &body));
        if content.is_empty() || content.contains(ENVELOPE_TAG) {
            continue;
        }
        let claude_type = meta.kind.unwrap_or_default();
        let kind = map_kind(&claude_type, &format!("{content}\n{body}"));
        out.push(Mapped {
            kind,
            content,
            file: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            claude_type,
        });
    }
    out
}

/// The `legacy_imports` name of one Claude memory directory.
fn source_name(dir: &Path) -> String {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    format!("claude-memory:{}", dir.display())
}

/// The Claude memory directories of `project_path`'s scope that exist: those
/// of the scope root, every worktree, and the launch directory (see the
/// module docs), deduplicated.
pub fn claude_memory_dirs(project_path: &str) -> Vec<PathBuf> {
    let dir = Path::new(project_path);
    let mut launch: Vec<PathBuf> = vec![atlas_checkpoint::git::scope_root(dir)];
    launch.extend(atlas_checkpoint::git::worktree_paths(dir));
    launch.push(dir.to_path_buf());
    let mut seen = HashSet::new();
    launch
        .into_iter()
        .map(|d| claude_memory_dir(d.to_string_lossy().trim_end_matches('/')))
        .filter(|m| m.is_dir())
        .filter(|m| seen.insert(m.canonicalize().unwrap_or_else(|_| m.clone())))
        .collect()
}

// ── Preview and confirm ──────────────────────────────────────────────────────

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// What importing `dirs` into `project_path`'s scope would write. Reads
/// only: nothing in the record changes.
pub fn claude_import_preview(project_path: &str, dirs: &[PathBuf]) -> Result<ClaudeImportPreview, String> {
    let store = store_for(project_path)?;
    let mut lines: Vec<ImportLine> = Vec::new();
    // Line id → its index in `lines`: one line per id across sources.
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut all_imported = !dirs.is_empty();
    for dir in dirs {
        let imported = store.import_recorded(&source_name(dir)).map_err(err)?;
        all_imported &= imported;
        for m in read_dir(dir) {
            let id = line_id(m.kind, &m.content);
            let is_new = !imported && !store.holds_content(m.kind, &m.content).map_err(err)?;
            if let Some(&at) = seen.get(&id) {
                // The same memory in two sources: new if either offers it.
                lines[at].is_new |= is_new;
                continue;
            }
            seen.insert(id.clone(), lines.len());
            lines.push(ImportLine {
                id,
                kind: m.kind,
                content: m.content,
                file: m.file,
                claude_type: m.claude_type,
                is_new,
            });
        }
    }
    Ok(ClaudeImportPreview {
        sources: dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect(),
        already_imported: all_imported,
        lines,
    })
}

/// Import the new lines of `dirs` whose preview id is in `ids`: confidence
/// 0.7, source `import:claude`, through the record's usual write (redacted,
/// hash identity, near-duplicate merge). Records every source read as
/// imported — lines the user left unticked are not offered again — and
/// announces the change. Returns how many lines were stored anew (a line
/// merged into a near-duplicate does not count). Empty `ids` is a no-op.
pub fn claude_import_confirm(
    memory: &SharedMemoryStore,
    project_path: &str,
    dirs: &[PathBuf],
    ids: &[String],
) -> Result<usize, String> {
    // Nothing kept is not a consent to import: no write, no gate.
    if ids.is_empty() {
        return Ok(0);
    }
    let preview = claude_import_preview(project_path, dirs)?;
    let store = store_for(project_path)?;
    let now = memory.now();
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut written = 0;
    let mut kinds: Vec<&str> = Vec::new();
    for line in preview.lines.into_iter().filter(|l| l.is_new && wanted.contains(l.id.as_str())) {
        let kind = line.kind;
        let outcome = store
            .upsert_outcome(NewEntry {
                kind,
                key: String::new(),
                content: line.content,
                source: CLAUDE_IMPORT_SOURCE.to_string(),
                agent: String::new(),
                session_id: String::new(),
                confidence: CLAUDE_IMPORT_CONFIDENCE,
                at: now,
            })
            .map_err(err)?
            .outcome;
        // A near-duplicate of a stored memory merges into it: nothing new.
        if outcome == WriteOutcome::Merged {
            continue;
        }
        written += 1;
        if !kinds.contains(&kind.as_str()) {
            kinds.push(kind.as_str());
        }
    }
    for dir in dirs {
        store.mark_imported(&source_name(dir), now).map_err(err)?;
    }
    if !kinds.is_empty() {
        memory.announce(&store, &kinds);
    }
    Ok(written)
}

// ── Tauri commands ───────────────────────────────────────────────────────────

/// Preview importing the project's Claude auto-memory. Writes nothing.
#[tauri::command]
pub async fn memory_claude_import_preview(project_path: String) -> Result<ClaudeImportPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dirs = claude_memory_dirs(&project_path);
        claude_import_preview(&project_path, &dirs)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Import the previewed lines the user kept (`ids`). Returns how many were
/// written.
#[tauri::command]
pub async fn memory_claude_import_confirm(
    project_path: String,
    ids: Vec<String>,
    store: State<'_, SharedMemoryStore>,
    registry: State<'_, std::sync::Arc<super::memory_indexer::MemoryRegistry>>,
) -> Result<usize, String> {
    let store = store.inner().clone();
    let cwd = project_path.clone();
    let written = tauri::async_runtime::spawn_blocking(move || {
        let dirs = claude_memory_dirs(&project_path);
        claude_import_confirm(&store, &project_path, &dirs, &ids)
    })
    .await
    .map_err(|e| e.to_string())??;
    if written > 0 {
        registry.enqueue_index(&cwd);
    }
    Ok(written)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {

    /// A prompt as Atlas used to send it while context was pushed: the
    /// envelope in front of the user's words. Readers must still strip it
    /// from files written back then.
    fn legacy_wire_prompt(block: &str, user_text: &str) -> String {
        let envelope = atlas_agent_transcript::wrap_memory_envelope(&[block]).expect("a present block");
        format!("{envelope}\n\n{user_text}")
    }
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::commands::shared_memory::MemoryChanged;

    fn scratch(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("atlas-claude-import-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A Claude memory dir with one file per `(file, type, description, body)`.
    fn claude_dir(files: &[(&str, &str, &str, &str)]) -> PathBuf {
        let dir = scratch("claude").join("memory");
        std::fs::create_dir_all(&dir).unwrap();
        for (file, ty, description, body) in files {
            let desc = if description.is_empty() { String::new() } else { format!("description: {description}\n") };
            std::fs::write(
                dir.join(file),
                format!("---\nname: {file}\n{desc}metadata:\n  node_type: memory\n  type: {ty}\n---\n\n{body}\n"),
            )
            .unwrap();
        }
        std::fs::write(dir.join("MEMORY.md"), "# Memory Index\n- [x](x.md) — index line\n").unwrap();
        dir
    }

    fn sample() -> PathBuf {
        claude_dir(&[
            ("prefs.md", "user", "Prefers small PRs", "The user prefers small PRs."),
            ("no-mocks.md", "feedback", "Integration tests hit a real database", "Why: mocks hid a bug."),
            ("jwt.md", "project", "JWT signing uses RS256 instead of HS256", "Decided on 2026-07-14."),
            ("freeze.md", "project", "Merge freeze begins 2026-03-05", "Mobile release cut."),
            ("dash.md", "reference", "Latency dashboard is grafana.internal/d/api", ""),
        ])
    }

    fn project() -> String {
        scratch("project").to_string_lossy().into_owned()
    }

    fn kinds(p: &ClaudeImportPreview) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> =
            p.lines.iter().map(|l| (l.file.clone(), l.kind.as_str().to_string())).collect();
        out.sort();
        out
    }

    #[test]
    fn a_choice_is_a_decision_verb_or_an_either_or() {
        for yes in [
            "JWT signing uses RS256 instead of HS256",
            "We decided to drop the cache",
            "Chose Postgres over MySQL",
            "The team will use bun",
            "Prefer small PRs over big ones",
            "Locked: six kinds stay",
        ] {
            assert!(states_a_choice(yes), "{yes:?} states a choice");
        }
        for no in [
            "Merge freeze begins 2026-03-05",
            "The staging DB is on port 6543",
            "Preferences live in ~/.atlas",
            "Local config was wiped on 08-14",
        ] {
            assert!(!states_a_choice(no), "{no:?} states no choice");
        }
    }

    #[test]
    fn claude_types_map_to_kinds() {
        assert_eq!(map_kind("feedback", "we decided X"), EntryKind::Fact);
        assert_eq!(map_kind("reference", "chose Y"), EntryKind::Fact);
        assert_eq!(map_kind("user", "prefers Z"), EntryKind::Fact);
        assert_eq!(map_kind("project", "use RS256 instead of HS256"), EntryKind::Decision);
        assert_eq!(map_kind("project", "freeze begins Thursday"), EntryKind::Fact);
        assert_eq!(map_kind("", "anything"), EntryKind::Fact);
    }

    /// The preview lists each mapped line with its kind, and writes nothing.
    #[test]
    fn the_preview_lists_each_line_with_its_kind_and_writes_nothing() {
        let (store, p, dir) = (SharedMemoryStore::new(), project(), sample());
        let heard = Arc::new(Mutex::new(Vec::<MemoryChanged>::new()));
        store.on_change({
            let heard = heard.clone();
            Arc::new(move |c: &MemoryChanged| heard.lock().push(c.clone()))
        });

        let preview = claude_import_preview(&p, &[dir.clone()]).unwrap();
        assert_eq!(
            kinds(&preview),
            [
                ("dash.md", "fact"),
                ("freeze.md", "fact"),
                ("jwt.md", "decision"),
                ("no-mocks.md", "fact"),
                ("prefs.md", "fact"),
            ]
            .map(|(f, k)| (f.to_string(), k.to_string()))
            .to_vec(),
            "MEMORY.md, the index, is not a memory"
        );
        assert!(preview.lines.iter().all(|l| l.is_new));
        assert!(!preview.already_imported);
        let jwt = preview.lines.iter().find(|l| l.file == "jwt.md").unwrap();
        assert_eq!((jwt.content.as_str(), jwt.claude_type.as_str()), ("JWT signing uses RS256 instead of HS256", "project"));

        // Cancel = never confirming: the record is untouched.
        assert!(store.entries(&p).is_empty());
        assert!(heard.lock().is_empty());
        let again = claude_import_preview(&p, &[dir]).unwrap();
        assert!(again.lines.iter().all(|l| l.is_new), "a preview records no import");
    }

    /// Confirm writes the kept lines with import provenance and announces them.
    #[test]
    fn confirm_writes_the_kept_lines_with_import_provenance() {
        let (store, p, dir) = (SharedMemoryStore::new(), project(), sample());
        let heard = Arc::new(Mutex::new(Vec::<MemoryChanged>::new()));
        store.on_change({
            let heard = heard.clone();
            Arc::new(move |c: &MemoryChanged| heard.lock().push(c.clone()))
        });
        let preview = claude_import_preview(&p, &[dir.clone()]).unwrap();
        // The user unticks the dashboard line.
        let ids: Vec<String> = preview.lines.iter().filter(|l| l.file != "dash.md").map(|l| l.id.clone()).collect();

        assert_eq!(claude_import_confirm(&store, &p, &[dir], &ids).unwrap(), 4);

        let entries = store.entries(&p);
        assert_eq!(entries.len(), 4);
        for e in &entries {
            assert_eq!((e.source.as_str(), e.agent.as_str(), e.confidence), ("import:claude", "", 0.7), "{e:?}");
        }
        let jwt = entries.iter().find(|e| e.content.contains("RS256")).unwrap();
        assert_eq!(jwt.kind, "decision");
        assert!(entries.iter().all(|e| !e.content.contains("grafana")), "an unticked line is not written");
        let heard = heard.lock().clone();
        assert_eq!(heard.len(), 1, "one change for the whole import");
        let mut announced = heard[0].kinds.clone();
        announced.sort();
        assert_eq!(announced, vec!["decision".to_string(), "fact".to_string()]);
    }

    /// Once per source: the second import shows nothing new and writes nothing
    /// — even after the user forgot an imported line.
    #[test]
    fn a_second_import_of_the_same_source_is_a_no_op() {
        let (store, p, dir) = (SharedMemoryStore::new(), project(), sample());
        let ids = |pv: &ClaudeImportPreview| pv.lines.iter().map(|l| l.id.clone()).collect::<Vec<_>>();
        let first = claude_import_preview(&p, &[dir.clone()]).unwrap();
        assert_eq!(claude_import_confirm(&store, &p, &[dir.clone()], &ids(&first)).unwrap(), 5);
        let forgotten = store.entries(&p)[0].id;
        store.forget_entry(&p, forgotten).unwrap();

        let second = claude_import_preview(&p, &[dir.clone()]).unwrap();
        assert!(second.already_imported);
        assert!(second.lines.iter().all(|l| !l.is_new), "{:?}", second.lines);
        assert_eq!(claude_import_confirm(&store, &p, &[dir], &ids(&second)).unwrap(), 0);
        assert_eq!(store.entries(&p).len(), 4);
    }

    /// Without the gate row (another scope's import, a hand-written entry), a
    /// line the record already holds is still not new.
    #[test]
    fn a_line_the_record_already_holds_is_not_new() {
        let (p, dir) = (project(), sample());
        crate::commands::shared_memory::store_for(&p)
            .unwrap()
            .upsert(NewEntry {
                kind: EntryKind::Fact,
                key: String::new(),
                content: "prefers   small PRs".into(),
                source: "claude-code".into(),
                agent: "claude-code".into(),
                session_id: String::new(),
                confidence: 1.0,
                at: 1,
            })
            .unwrap();
        let preview = claude_import_preview(&p, &[dir]).unwrap();
        let prefs = preview.lines.iter().find(|l| l.file == "prefs.md").unwrap();
        assert!(!prefs.is_new);
        assert_eq!(preview.lines.iter().filter(|l| l.is_new).count(), 4);
    }

    /// Enveloped text inside a Claude memory file is not imported.
    #[test]
    fn enveloped_text_is_not_imported() {
        let (store, p) = (SharedMemoryStore::new(), project());
        let injected = legacy_wire_prompt(
            "--- SHARED MEMORY ---\n[DECISIONS]\n- Use RS256 (by codex)\n--- END SHARED MEMORY ---",
            "The team prefers bun over npm.",
        );
        // No description: the line comes from the body, which Claude saved
        // with Atlas's envelope still around the prompt.
        let dir = claude_dir(&[("recycled.md", "project", "", &injected)]);

        let preview = claude_import_preview(&p, &[dir.clone()]).unwrap();
        assert_eq!(preview.lines.len(), 1);
        let line = &preview.lines[0];
        assert_eq!(line.content, "The team prefers bun over npm.");
        let ids = vec![line.id.clone()];
        claude_import_confirm(&store, &p, &[dir], &ids).unwrap();
        for e in store.entries(&p) {
            for leaked in ["<atlas-memory>", "SHARED MEMORY", "Use RS256", "Do not save any of it"] {
                assert!(!e.content.contains(leaked), "{leaked:?} was imported");
            }
        }
    }

    /// A body-only memory contributes its first paragraph, headings skipped.
    #[test]
    fn a_memory_without_a_description_contributes_its_first_paragraph() {
        assert_eq!(line_of("", "# Title\n\nFirst line\ncontinues.\n\nSecond paragraph."), "First line continues.");
        assert_eq!(line_of("  The summary  ", "Body."), "The summary");
    }

    /// Claude's documented top-level `type:` is read as well as the nested one.
    #[test]
    fn a_top_level_type_is_read() {
        let dir = scratch("top-level").join("memory");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "---\nname: a\ndescription: Chose Fly over Render\ntype: project\n---\n\nbody").unwrap();
        let lines = read_dir(&dir);
        assert_eq!((lines[0].kind, lines[0].claude_type.as_str()), (EntryKind::Decision, "project"));
    }

    /// Confirming with nothing kept writes nothing and does not use up the
    /// source.
    #[test]
    fn confirming_nothing_leaves_the_source_importable() {
        let (store, p, dir) = (SharedMemoryStore::new(), project(), sample());
        assert_eq!(claude_import_confirm(&store, &p, &[dir.clone()], &[]).unwrap(), 0);
        let preview = claude_import_preview(&p, &[dir]).unwrap();
        assert!(!preview.already_imported && preview.lines.iter().all(|l| l.is_new));
    }

    /// The same memory in an imported source and a fresh one is offered once,
    /// as new.
    #[test]
    fn a_memory_in_two_sources_is_one_line() {
        let (store, p) = (SharedMemoryStore::new(), project());
        let old = claude_dir(&[("a.md", "user", "Prefers small PRs", "")]);
        claude_import_confirm(&store, &p, &[old.clone()], &["nothing-kept".into()]).unwrap();
        let fresh = claude_dir(&[("b.md", "user", "Prefers small PRs", "")]);
        let preview = claude_import_preview(&p, &[old, fresh]).unwrap();
        assert_eq!(preview.lines.len(), 1);
        assert!(preview.lines[0].is_new);
    }

    /// No Claude memory for the scope: nothing to preview, nothing imported.
    #[test]
    fn no_source_previews_nothing() {
        let (store, p) = (SharedMemoryStore::new(), project());
        let preview = claude_import_preview(&p, &[]).unwrap();
        assert!(preview.lines.is_empty() && preview.sources.is_empty() && !preview.already_imported);
        assert_eq!(claude_import_confirm(&store, &p, &[], &[]).unwrap(), 0);
    }
}
