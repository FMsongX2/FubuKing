//! Source-level audit: every `Command::new(` in Windows-reachable code must
//! opt out of a console window, or be listed below as a known gap.
//!
//! Atlas is a GUI (console-less) process. On Windows, any console-subsystem
//! child spawned without `CREATE_NO_WINDOW` gets a fresh console, and with
//! Windows Terminal as the default terminal host that console is a visible
//! window (see docs/archive/windows-terminal-spawn.md). The compiler cannot
//! catch a missing flag, so this test walks the source instead.
//!
//! A spawn site counts as gated when, within `WINDOW` lines after
//! `Command::new(`, the code calls one of `GATES` (the `atlas-process`
//! helper, the atlas-agent git-utils helper, a raw `creation_flags`, or the
//! `quiet(..)` wrapper in atlas-engine-shell-command).
//!
//! `KNOWN_GAPS` is the action-plan backlog: paths that still spawn without a
//! gate. The test fails when a NEW ungated site appears, and it also fails
//! when a listed gap has been fixed, so the list is pruned as work lands.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

const WINDOW: usize = 30;
const GATES: &[&str] = &[
    ".no_window()",
    "no_console_window(",
    ".creation_flags(",
    "quiet(&mut",
    // atlas-engine-utils-pty's job object spawns with CREATE_NO_WINDOW itself.
    ".spawn_contained(",
    ".prepare_suspended_spawn(",
    // atlas-engine-git-utils runs every git command through the job object.
    "run_git_command_with_timeout",
];

/// Ungated spawn sites that are reachable on Windows and still open for work.
/// Keep in sync with the action plan in docs/archive/windows-terminal-spawn.md.
const KNOWN_GAPS: &[&str] = &[
    // Runs inside the sandbox `command_runner` binary, itself a console
    // process, so its `cmd.exe` child inherits that console: no new window.
    "vendor/atlas-engine/windows-sandbox-rs/src/bin/command_runner/win/cwd_junction.rs",
];

/// Files that only ever run off Windows, or only in tests/tooling.
fn skipped(path: &str) -> bool {
    const FRAGMENTS: &[&str] = &[
        "/tests/",
        "/testing/",
        "_tests.rs",
        "/tests.rs",
        "/benches/",
        "/examples/",
        "/build.rs",
        "/seatbelt",
        "/landlock",
        "/bwrap",
        "/unix/",
        "/linux",
        "macos",
        "/escalate_server",
        "/process_group",
        "/tmux",
        "/zellij",
        // Test fixtures and dev tooling (prettier over generated TS, the
        // stdio test server, the wine runners) never run inside Atlas.
        "/test_stdio_server",
        "/wine_",
        "app-server-protocol/src/export.rs",
        "app-server-protocol/src/precomputed_exports.rs",
    ];
    FRAGMENTS.iter().any(|f| path.contains(f))
}

fn unix_only_file(src: &str) -> bool {
    src.lines().take(40).any(|l| {
        let l = l.trim();
        l.starts_with("#![cfg(unix)]")
            || l.starts_with("#![cfg(not(windows))]")
            || l.starts_with("#![cfg(target_os = \"macos\")]")
            || l.starts_with("#![cfg(target_os = \"linux\")]")
            || l.starts_with("#![cfg(any(target_os = \"macos\"")
            || l.starts_with("#![cfg(any(target_os = \"linux\"")
    })
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == "node_modules" || name.starts_with('.') {
                continue;
            }
            walk(&path, out);
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// `Command::new(` for a process command — `std`/`tokio` `Command` or one of
/// the aliases the workspace uses for them — and not `GitCommand::new(`,
/// `AvailableCommand::new(` and the like.
fn is_process_command_new(line: &str) -> bool {
    const ALIASES: &[&str] = &["StdCommand", "AsyncCommand", "TokioCommand"];
    let mut rest = line;
    while let Some(pos) = rest.find("Command::new(") {
        let before = &rest[..pos];
        let ident_start = before
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map_or(0, |i| i + 1);
        let prefix = &before[ident_start..];
        if prefix.is_empty() || ALIASES.contains(&format!("{prefix}Command").as_str()) {
            return true;
        }
        rest = &rest[pos + "Command::new(".len()..];
    }
    false
}

fn ungated_sites_in(src: &str) -> Vec<usize> {
    let lines: Vec<&str> = src.lines().collect();
    let mut sites = Vec::new();
    let mut in_tests = false;
    let mut cfg_stack: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        // Everything after the first `#[cfg(test)]`-gated item is test code;
        // the test module is conventionally last in the file.
        if (trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[cfg(all(test"))
            && lines[i + 1..(i + 4).min(lines.len())]
                .iter()
                .any(|l| l.trim_start().starts_with("mod "))
        {
            in_tests = true;
        }
        // A spawn directly under a unix-only cfg attribute is unreachable on
        // Windows. Track the attribute for the next item only.
        let unix_attr = trimmed.starts_with("#[cfg(unix)]")
            || trimmed.starts_with("#[cfg(not(windows))]")
            || trimmed.starts_with("#[cfg(target_os = \"macos\")]")
            || trimmed.starts_with("#[cfg(target_os = \"linux\")]")
            || trimmed.starts_with("#[cfg(any(target_os = \"macos\"")
            || trimmed.starts_with("#[cfg(any(target_os = \"linux\"");
        if unix_attr {
            cfg_stack.push(true);
            i += 1;
            continue;
        }
        let unix_item = cfg_stack.pop().unwrap_or(false);
        if unix_item {
            // Skip the whole item: up to the end of its brace block, or to
            // the `;` of a brace-less item (`use`, a one-line statement).
            let mut depth = 0i32;
            let mut opened = false;
            let mut j = i;
            loop {
                let l = lines[j];
                if !opened && l.contains(';') && !l.contains('{') {
                    break;
                }
                depth += l.matches('{').count() as i32;
                depth -= l.matches('}').count() as i32;
                opened |= l.contains('{');
                if opened && depth <= 0 {
                    break;
                }
                if j + 1 >= lines.len() {
                    break;
                }
                j += 1;
            }
            i = j + 1;
            continue;
        }
        if !in_tests && is_process_command_new(line) && !trimmed.starts_with("//") {
            let end = (i + WINDOW).min(lines.len());
            let region = lines[i..end].join("\n");
            if !GATES.iter().any(|g| region.contains(g)) {
                sites.push(i + 1);
            }
        }
        i += 1;
    }
    sites
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

#[test]
fn every_windows_reachable_spawn_is_gated_or_a_known_gap() {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ["src-tauri/src", "crates", "vendor/atlas-engine"] {
        walk(&root.join(dir), &mut files);
    }
    assert!(files.len() > 100, "expected to walk the whole workspace");

    let mut new_gaps: Vec<String> = Vec::new();
    let mut still_open: Vec<&str> = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if skipped(&format!("/{rel}")) {
            continue;
        }
        let Ok(src) = fs::read_to_string(file) else {
            continue;
        };
        if unix_only_file(&src) {
            continue;
        }
        let sites = ungated_sites_in(&src);
        if sites.is_empty() {
            continue;
        }
        if let Some(known) = KNOWN_GAPS.iter().find(|k| **k == rel) {
            still_open.push(known);
        } else {
            for line in sites {
                new_gaps.push(format!("{rel}:{line}"));
            }
        }
    }

    let fixed: Vec<&&str> = KNOWN_GAPS
        .iter()
        .filter(|k| !still_open.contains(k))
        .collect();
    assert!(
        new_gaps.is_empty(),
        "ungated Command::new sites reachable on Windows (add CREATE_NO_WINDOW via \
         atlas_process::NoWindow / creation_flags, or list the file in KNOWN_GAPS):\n  {}",
        new_gaps.join("\n  ")
    );
    assert!(
        fixed.is_empty(),
        "KNOWN_GAPS entries are now gated — remove them from the list and the action plan: {fixed:?}"
    );
}
