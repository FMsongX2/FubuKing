import { describe, expect, it } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Apache-2.0 obligations Quotatlas takes on as a fork of Atlas.
 *
 * - §4(b): every upstream file Quotatlas modifies says so. Source files carry
 *   `NOTICE_LINE` within their first lines; files with no comment syntax are
 *   listed in `QUOTATLAS-CHANGES.md` instead.
 * - §4(a)/(d): the licence and Quotatlas's NOTICE reach recipients, which means
 *   the shipped bundle, not just the repository.
 *
 * `FORK_BASE` is the last upstream commit merged into Quotatlas. When an
 * upstream release is merged, move it to that release's commit in the same
 * merge, otherwise every file upstream touched reads as a Quotatlas change.
 * CI must check out full history (`fetch-depth: 0`) for the diff to resolve.
 */

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const FORK_BASE = "a34a6d44bf37d26d9a6f8f6fe1fab5ce0a92d8d1";
const NOTICE_LINE = "Modified by Quotatlas from upstream Atlas (Apache-2.0).";
const CHANGES_FILE = "QUOTATLAS-CHANGES.md";
/** How far down a file the notice may sit: a shebang or doctype can precede it. */
const NOTICE_WINDOW = 3;
const TAURI_DIR = path.join(REPO_ROOT, "src-tauri");

function git(...args: string[]): string {
  return execFileSync("git", args, { cwd: REPO_ROOT, encoding: "utf8" });
}

function read(rel: string): string {
  return readFileSync(path.join(REPO_ROOT, rel), "utf8");
}

/** Upstream files that differ from the fork base, committed or not. */
function modifiedUpstreamFiles(): string[] {
  return git("diff", "--name-only", "--diff-filter=M", FORK_BASE)
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

/** Paths `QUOTATLAS-CHANGES.md` lists, one backticked path per bullet. */
function listedChanges(): string[] {
  return read(CHANGES_FILE)
    .split("\n")
    .map((line) => /^- `([^`]+)`/.exec(line)?.[1])
    .filter((rel): rel is string => Boolean(rel));
}

describe("Quotatlas marks every upstream file it modifies (Apache-2.0 §4(b))", () => {
  it("still has the fork base in its history", () => {
    expect(() => git("cat-file", "-e", `${FORK_BASE}^{commit}`)).not.toThrow();
  });

  it("puts the notice near the top of each modified file, or lists the file", () => {
    const listed = new Set(listedChanges());
    const unmarked = modifiedUpstreamFiles().filter((rel) => {
      if (listed.has(rel)) return false;
      const head = read(rel).split("\n").slice(0, NOTICE_WINDOW).join("\n");
      return !head.includes(NOTICE_LINE);
    });
    expect(unmarked, `add "${NOTICE_LINE}" or list the file in ${CHANGES_FILE}`).toEqual([]);
  });

  it("lists only files that really differ from upstream", () => {
    const modified = new Set(modifiedUpstreamFiles());
    const stale = listedChanges().filter((rel) => !modified.has(rel));
    expect(stale, `${CHANGES_FILE} names files Quotatlas no longer modifies`).toEqual([]);
  });
});

describe("the licence and NOTICE ship with the app (Apache-2.0 §4(a)/(d))", () => {
  const resources = (): string[] =>
    Object.keys(JSON.parse(read("src-tauri/tauri.conf.json")).bundle.resources as object);

  it("keeps a NOTICE that credits upstream Atlas", () => {
    expect(existsSync(path.join(REPO_ROOT, "NOTICE"))).toBe(true);
    expect(read("NOTICE")).toMatch(/modified version of Atlas/);
    expect(read("NOTICE")).toMatch(/Copyright 2026 Adib Mohsin/);
  });

  it("bundles byte-identical copies of LICENSE and NOTICE", () => {
    expect(resources()).toContain("licenses/Quotatlas-LICENSE.txt");
    expect(resources()).toContain("licenses/Quotatlas-NOTICE.txt");
    expect(readFileSync(path.join(TAURI_DIR, "licenses/Quotatlas-LICENSE.txt"), "utf8")).toBe(
      read("LICENSE"),
    );
    expect(readFileSync(path.join(TAURI_DIR, "licenses/Quotatlas-NOTICE.txt"), "utf8")).toBe(
      read("NOTICE"),
    );
  });
});
