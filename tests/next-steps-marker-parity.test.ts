import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Keeps the four sites that know Atlas's next-steps directive in agreement.
 *
 * Atlas appends a hidden directive to every wire prompt so the agent ends its
 * reply with a `<next_steps>` block. Four places have to recognise it again
 * afterwards, and they are in three languages' worth of different crates:
 *
 *  - `src/features/chat/lib/next-steps.ts` **writes** it, and strips it back
 *    out of anything displayed.
 *  - `crates/atlas-checkpoint/src/capture.rs` strips it from the stored
 *    transcript, so the record holds what the developer and the agent actually
 *    said rather than what Atlas interjected.
 *  - `crates/atlas-acp-thread/src/thread.rs` rejects a *title* built out of it.
 *    That one is the bug this suite was written for: an agent that names a
 *    session by summarising its first prompt (Claude Code does) sees a short
 *    user message outweighed by Atlas's own words and answers "Atlas
 *    next-steps", which every surface then repeats back as if the user had
 *    said it.
 *
 * None of them can import from the others — the TS side is a different
 * language, and `atlas-checkpoint` deliberately depends on neither of the agent
 * crates (it is the Tauri-free record store). So the literal is copied, each
 * copy carrying a "must change together" comment, and *this* is the only thing
 * in the toolchain that can tell when one copy moved and the others did not.
 *
 * The failure is silent in the worst way: every gate stays green, and the
 * symptom is harness text leaking into a transcript, a title, or a chat bubble
 * — surfaces where nobody is looking for a regression.
 */

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** Each site, and the pattern that finds its copy of the literal. */
const SITES = [
  {
    file: "src/features/chat/lib/next-steps.ts",
    pattern: /const NEXT_STEPS_MARKER = "([^"]+)"/,
  },
  {
    file: "crates/atlas-checkpoint/src/capture.rs",
    pattern: /const NEXT_STEPS_MARKER: &str = "([^"]+)"/,
  },
  {
    file: "crates/atlas-acp-thread/src/thread.rs",
    pattern: /const NEXT_STEPS_MARKER: &str = "([^"]+)"/,
  },
] as const;

function markerIn(site: (typeof SITES)[number]): string {
  const source = readFileSync(path.join(REPO_ROOT, site.file), "utf8");
  const found = source.match(site.pattern);
  expect(found, `no NEXT_STEPS_MARKER declaration in ${site.file}`).not.toBeNull();
  return found![1];
}

describe("next-steps marker parity", () => {
  it("is the same literal everywhere it is declared", () => {
    const markers = SITES.map((site) => [site.file, markerIn(site)] as const);
    const distinct = new Set(markers.map(([, marker]) => marker));
    expect(
      distinct.size,
      `the marker has drifted:\n${markers.map(([f, m]) => `  ${f}: ${m}`).join("\n")}`,
    ).toBe(1);
  });

  it("is still what the directive actually emits", () => {
    // Parity between the copies is worth nothing if the *writer* stopped
    // emitting the string they all agree on — the strippers would then be
    // matching a marker that no prompt contains.
    const source = readFileSync(
      path.join(REPO_ROOT, "src/features/chat/lib/next-steps.ts"),
      "utf8",
    );
    const marker = markerIn(SITES[0]);
    expect(source).toMatch(
      new RegExp(`\\$\\{NEXT_STEPS_MARKER\\}|${marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`),
    );
  });

  it("still tells the agent not to title the session from it", () => {
    // The filter in `atlas-acp-thread` is the net; this line is what keeps the
    // bad title from being minted in the first place. Losing it in an edit to
    // the directive's wording would be invisible until a thread came back
    // misnamed again.
    const source = readFileSync(
      path.join(REPO_ROOT, "src/features/chat/lib/next-steps.ts"),
      "utf8",
    );
    expect(source.toLowerCase()).toMatch(/naming or titling this session/);
  });
});
