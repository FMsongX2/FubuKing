import { describe, expect, it } from "vitest";
import { Text } from "@codemirror/state";
import { revealRange } from "./reveal";

const doc = Text.of(["first line", "second", "", "fourth line here"]);

describe("revealRange", () => {
  it("puts the cursor at the start of a 1-based line", () => {
    const at = revealRange(doc, { line: 2 });
    expect(at).toEqual({ anchor: doc.line(2).from, head: doc.line(2).from });
  });

  it("honours a 1-based column", () => {
    const at = revealRange(doc, { line: 4, column: 3 });
    expect(at.anchor).toBe(doc.line(4).from + 2);
    expect(at.head).toBe(at.anchor);
  });

  /// The agent reads line numbers off stale output all the time; a line past
  /// the end must land on the last line, never throw inside CodeMirror.
  it("clamps a line past the end to the last line", () => {
    expect(revealRange(doc, { line: 999 }).anchor).toBe(doc.line(4).from);
  });

  it("clamps a line below 1 to the first line", () => {
    expect(revealRange(doc, { line: 0 }).anchor).toBe(0);
  });

  it("clamps a column past the end of its line", () => {
    expect(revealRange(doc, { line: 2, column: 50 }).anchor).toBe(doc.line(2).to);
  });

  it("selects to the end of the end line when no end column is given", () => {
    const at = revealRange(doc, { line: 1, endLine: 2 });
    expect(at).toEqual({ anchor: 0, head: doc.line(2).to });
  });

  it("selects to an end column when one is given", () => {
    const at = revealRange(doc, { line: 1, column: 2, endLine: 4, endColumn: 5 });
    expect(at).toEqual({ anchor: 1, head: doc.line(4).from + 4 });
  });

  it("copes with an empty document", () => {
    expect(revealRange(Text.of([""]), { line: 5, column: 5 })).toEqual({ anchor: 0, head: 0 });
  });
});
