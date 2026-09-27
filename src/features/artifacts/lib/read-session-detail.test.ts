import { describe, expect, it, vi } from "vitest";

import { readSessionDetail, type DetailSources } from "./read-session-detail";
import type { SessionDetail } from "../types";

function detail(id: string): SessionDetail {
  return {
    summary: { id } as SessionDetail["summary"],
    entries: [],
    counts: { prompts: 0, responses: 0, thinking: 0, toolCalls: 0, checkpoints: 0 },
    tools: [],
  };
}

function sources(over: Partial<DetailSources> = {}): DetailSources {
  return {
    local: vi.fn(async () => null),
    remote: vi.fn(async (_p: string, id: string) => detail(id)),
    ...over,
  };
}

describe("readSessionDetail", () => {
  it("prefers the local store when the Session is in it", async () => {
    const io = sources({ local: vi.fn(async (_p, id) => detail(id)) });
    const out = await readSessionDetail(
      { sessionId: "ses_1", projectPath: "/repo", remoteProjectId: "ws_1" },
      io,
    );
    expect(out?.summary.id).toBe("ses_1");
    expect(io.remote).not.toHaveBeenCalled();
  });

  it("falls back to the server for a teammate's Session in a Project this machine has", async () => {
    // The bug: the board gives every Session of a bound Project a local
    // `projectPath`, including rows that only ever existed on the server. A
    // local miss must not be the end of the story.
    const io = sources();
    const out = await readSessionDetail(
      { sessionId: "ses_theirs", projectPath: "/repo", remoteProjectId: "ws_1" },
      io,
    );
    expect(io.local).toHaveBeenCalledWith("/repo", "ses_theirs");
    expect(io.remote).toHaveBeenCalledWith("ws_1", "ses_theirs");
    expect(out?.summary.id).toBe("ses_theirs");
  });

  it("reads straight from the server when there is no checkout", async () => {
    const io = sources();
    await readSessionDetail({ sessionId: "ses_2", projectPath: "", remoteProjectId: "ws_1" }, io);
    expect(io.local).not.toHaveBeenCalled();
    expect(io.remote).toHaveBeenCalledWith("ws_1", "ses_2");
  });

  it("answers null, without asking the server, for a local-only Session that is gone", async () => {
    const io = sources();
    const out = await readSessionDetail(
      { sessionId: "ses_3", projectPath: "/repo", remoteProjectId: null },
      io,
    );
    expect(out).toBeNull();
    expect(io.remote).not.toHaveBeenCalled();
  });

  it("lets a server refusal surface as an error rather than as absence", async () => {
    const io = sources({
      remote: vi.fn(async () => {
        throw new Error("session: not found");
      }),
    });
    await expect(
      readSessionDetail({ sessionId: "ses_4", projectPath: "", remoteProjectId: "ws_1" }, io),
    ).rejects.toThrow("not found");
  });
});
