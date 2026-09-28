//! Apache-2.0 obligations FubuKing takes on as a modified version of Atlas.
//!
//! - §4(b): every upstream file FubuKing modifies says so. Source files carry
//!   `NOTICE_LINE` within their first lines; files with no comment syntax are
//!   listed in `FUBUKING-CHANGES.md` instead.
//! - §4(d): the NOTICE credits upstream Atlas. The release archives carry it
//!   beside the binary (`.github/workflows/release-cli.yml`).
//!
//! `FORK_BASE` is the upstream commit FubuKing forked from. When upstream
//! changes are taken in, move it in the same change, otherwise every file
//! upstream touched reads as a FubuKing change. Files FubuKing deleted are not
//! checked: they are not distributed. The diff needs full history, so CI
//! checks out with `fetch-depth: 0`.

use std::path::{Path, PathBuf};
use std::process::Command;

const FORK_BASE: &str = "a34a6d44bf37d26d9a6f8f6fe1fab5ce0a92d8d1";
const NOTICE_LINE: &str = "Modified by FubuKing from upstream Atlas (Apache-2.0).";
const CHANGES_FILE: &str = "FUBUKING-CHANGES.md";
/// How far down a file the notice may sit: a shebang or doctype can precede it.
const NOTICE_WINDOW: usize = 3;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn git(args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(repo_root()).output().expect("running git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).expect("git prints UTF-8")
}

fn read(rel: &str) -> String {
    let bytes = std::fs::read(repo_root().join(rel)).unwrap_or_else(|error| panic!("reading {rel}: {error}"));
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Upstream files that differ from the fork base, committed or not.
fn modified_upstream_files() -> Vec<String> {
    git(&["diff", "-z", "--name-only", "--diff-filter=M", FORK_BASE])
        .split('\0')
        .filter(|rel| !rel.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Paths `FUBUKING-CHANGES.md` lists, one backticked path per bullet. A path
/// ending in `/` is a directory entry that covers every file under it.
fn listed_changes() -> Vec<String> {
    read(CHANGES_FILE)
        .lines()
        .filter_map(|line| line.strip_prefix("- `")?.split_once('`'))
        .map(|(rel, _)| rel.to_owned())
        .filter(|rel| !rel.is_empty())
        .collect()
}

fn covers(entry: &str, rel: &str) -> bool {
    if entry.ends_with('/') {
        rel.starts_with(entry)
    } else {
        rel == entry
    }
}

#[test]
fn the_fork_base_is_in_history() {
    git(&["cat-file", "-e", &format!("{FORK_BASE}^{{commit}}")]);
}

#[test]
fn every_modified_upstream_file_carries_the_notice_or_is_listed() {
    let listed = listed_changes();
    let unmarked: Vec<String> = modified_upstream_files()
        .into_iter()
        .filter(|rel| !listed.iter().any(|entry| covers(entry, rel)))
        .filter(|rel| !read(rel).lines().take(NOTICE_WINDOW).any(|line| line.contains(NOTICE_LINE)))
        .collect();
    assert!(unmarked.is_empty(), "add \"{NOTICE_LINE}\" or list the file in {CHANGES_FILE}: {unmarked:?}");
}

#[test]
fn the_changes_file_lists_only_files_that_differ_from_upstream() {
    let modified = modified_upstream_files();
    let stale: Vec<String> = listed_changes()
        .into_iter()
        .filter(|entry| !modified.iter().any(|rel| covers(entry, rel)))
        .collect();
    assert!(stale.is_empty(), "{CHANGES_FILE} names files FubuKing no longer modifies: {stale:?}");
}

#[test]
fn the_notice_credits_upstream_atlas() {
    let notice = read("NOTICE");
    assert!(notice.contains("modified version of Atlas"));
    assert!(notice.contains("Copyright 2026 Adib Mohsin"));
}
