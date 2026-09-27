import { describe, expect, it } from "vitest";
import { EditorSelection, EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { editorPosition, getEditorView, registerEditorView } from "./editor-views";

const fakeView = (state: EditorState) => ({ state }) as unknown as EditorView;

describe("editor view registry", () => {
  it("returns a registered view and forgets it on unregister", () => {
    const view = fakeView(EditorState.create({ doc: "a" }));
    const unregister = registerEditorView("editor:/a.ts", view);
    expect(getEditorView("editor:/a.ts")).toBe(view);
    unregister();
    expect(getEditorView("editor:/a.ts")).toBeUndefined();
  });

  /// A tab remounted before the old panel's cleanup ran must not have its new
  /// view dropped by the stale unregister.
  it("an old unregister does not drop a newer view for the same tab", () => {
    const first = fakeView(EditorState.create({ doc: "a" }));
    const second = fakeView(EditorState.create({ doc: "b" }));
    const dropFirst = registerEditorView("editor:/a.ts", first);
    registerEditorView("editor:/a.ts", second);
    dropFirst();
    expect(getEditorView("editor:/a.ts")).toBe(second);
  });
});

describe("editorPosition", () => {
  const doc = "alpha\nbeta\ngamma";

  it("reports a 1-based cursor and no selection when the selection is empty", () => {
    const state = EditorState.create({ doc, selection: EditorSelection.cursor(8) });
    expect(editorPosition(state)).toEqual({ cursor: { line: 2, col: 3 } });
  });

  it("reports the selection's start and end when it is not empty", () => {
    const state = EditorState.create({ doc, selection: EditorSelection.range(1, 13) });
    expect(editorPosition(state)).toEqual({
      cursor: { line: 3, col: 3 },
      selection: { from: { line: 1, col: 2 }, to: { line: 3, col: 3 } },
    });
  });
});
