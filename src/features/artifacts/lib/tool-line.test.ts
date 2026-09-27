import { describe, expect, it } from "vitest";

import { toolLine } from "./tool-line";
import type { TimelineEntry } from "../types";

function call(partial: Partial<TimelineEntry>): TimelineEntry {
  return {
    id: "tc-1",
    kind: "tool_call",
    at: "2026-09-27T10:00:00Z",
    turnSeq: 1,
    text: null,
    truncated: false,
    bodyBytes: 0,
    bodyRef: null,
    toolName: null,
    toolTitle: null,
    toolStatus: "completed",
    paths: [],
    arguments: null,
    argumentsRef: null,
    result: null,
    resultRef: null,
    resultBinary: false,
    commitSha: null,
    commitSubject: null,
    branch: null,
    linkState: null,
    insertions: 0,
    deletions: 0,
    files: [],
    ...partial,
  };
}

describe("toolLine", () => {
  it("reads a recorded Read the way the transcript does", () => {
    const line = toolLine(
      call({
        toolName: "Read",
        paths: ["src/features/chat/components/transcript-rows.tsx"],
        arguments: JSON.stringify({
          file_path: "src/features/chat/components/transcript-rows.tsx",
        }),
      }),
    );
    expect(line).toEqual({
      tool: "read",
      verb: "Read",
      // Two trailing segments, as `shortPath` trims them.
      detail: "components/transcript-rows.tsx",
      fileDetail: true,
    });
  });

  it("tells a created file from an edited one", () => {
    const created = toolLine(
      call({
        toolName: "Write",
        paths: ["src/new.ts"],
        arguments: JSON.stringify({ file_path: "src/new.ts", content: "a\nb\n" }),
      }),
    );
    expect(created.verb).toBe("Created");
    expect(created.tool).toBe("edit");

    const edited = toolLine(
      call({
        toolName: "Edit",
        paths: ["src/old.ts"],
        arguments: JSON.stringify({
          file_path: "src/old.ts",
          old_string: "one",
          new_string: "two",
        }),
      }),
    );
    expect(edited.verb).toBe("Edited");
  });

  it("reduces a recorded shell command to the action it really was", () => {
    // The same rule the transcript uses: `cat` is a read, not a command.
    const read = toolLine(
      call({
        toolName: "Bash",
        arguments: JSON.stringify({ command: "cat crates/a/src/main.rs" }),
      }),
    );
    expect(read.tool).toBe("read");
    expect(read.verb).toBe("Read");

    const ran = toolLine(
      call({ toolName: "Bash", arguments: JSON.stringify({ command: "cargo test  --all" }) }),
    );
    expect(ran).toEqual({
      tool: "run",
      verb: "Ran",
      // Whitespace collapsed, so a wrapped command stays one line.
      detail: "cargo test --all",
      fileDetail: false,
    });
  });

  it("speaks for an Other call with the title the agent gave it", () => {
    // `tools.rs` has no bucket for an MCP call, so the record says "Other" and
    // the ACP title is the only description of it there is.
    const line = toolLine(call({ toolName: "Other", toolTitle: "List issues" }));
    expect(line).toEqual({ tool: "tool", verb: "List issues", detail: "", fileDetail: false });

    // Nothing at all: still a sentence, never the bare word "Other".
    expect(toolLine(call({ toolName: "Other" })).verb).toBe("Used a tool");
  });

  it("survives arguments that spilled to a blob or never parsed", () => {
    // `arguments` is null when the payload was too large to inline; `paths` is
    // resolved by `tools.rs` and is still there, so the line still says what
    // the call touched.
    const spilled = toolLine(
      call({ toolName: "Read", paths: ["a/b/c.rs"], arguments: null, argumentsRef: "blob-1" }),
    );
    expect(spilled.detail).toBe("b/c.rs");

    const broken = toolLine(call({ toolName: "Search", arguments: "{not json" }));
    expect(broken.verb).toBe("Searched");
    expect(broken.tool).toBe("search");
  });
});
