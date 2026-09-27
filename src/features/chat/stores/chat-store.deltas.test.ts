// @vitest-environment happy-dom
//
// `applyAgentDelta`: the frontend's mirror of Rust's session log.
//
// Rust owns the transcript (`atlas-acp-thread`); the store holds a mirror kept
// current by the `atlas:agents` delta stream (`crates/atlas-agent-wire/src/
// delta.rs`, typed as `AgentDelta` in `src/types/agents.ts`). Every case here
// feeds a sequence of wire deltas through the store's real action and asserts
// the session the UI would render from — so a delta kind that stops being
// applied, or is applied to the wrong message, fails here rather than as a
// chat that silently drifts from what the agent said.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => {}),
}));

import type { AgentDelta, SessionMessage, ToolCall } from "@/types/agents";
import type { SwitchableAgent } from "@/types/agent";
import { useChatStore } from "./chat-store";

const TAB = "tab-1";
const AGENT = "agent-1";
const ACP = "acp-session-1";

type Kind = AgentDelta["kind"];
type Body<K extends Kind> = Omit<
  Extract<AgentDelta, { kind: K }>,
  "kind" | "agent_id" | "session_id"
>;

/** One wire delta for the bound session. */
function d<K extends Kind>(kind: K, body: Body<K>): AgentDelta {
  // One generic spread TypeScript cannot narrow back to the union member.
  return { kind, agent_id: AGENT, session_id: ACP, ...body } as unknown as AgentDelta;
}

function wireMessage(over: Partial<SessionMessage> & Pick<SessionMessage, "id">): SessionMessage {
  return {
    role: "assistant",
    mode: "text",
    content: "",
    tool_calls: [],
    timestamp: "2026-09-18T10:00:00Z",
    ...over,
  };
}

function wireTool(id: string, over: Partial<ToolCall> = {}): ToolCall {
  return {
    id,
    tool_name: "Bash",
    title: null,
    kind: "execute",
    status: "running",
    arguments: { command: "ls" },
    result: null,
    locations: [],
    ...over,
  };
}

const apply = (...deltas: AgentDelta[]) => {
  for (const delta of deltas) useChatStore.getState().actions.applyAgentDelta(delta);
};
const session = () => useChatStore.getState().sessions[TAB];
const messages = () => session().messages;
const last = () => messages()[messages().length - 1];

/** A bound tab with the user's prompt already in it — where every turn starts. */
function boundTab(agentType: SwitchableAgent = "claude-code", prompt = "list the files") {
  const { actions } = useChatStore.getState();
  actions.createSession(TAB, agentType);
  actions.setAcpBinding(TAB, AGENT, ACP, "/tmp/project");
  if (prompt) actions.addMessage(TAB, "user", prompt);
}

beforeEach(() => {
  localStorage.clear();
  useChatStore.setState({
    sessions: {},
    pendingPermissions: {},
    queues: {},
    activeSessionId: null,
  });
});

describe("applyAgentDelta: one delta kind at a time", () => {
  // Each row: what arrives, and what the session looks like after it. Rows
  // start from `boundTab()` — a user prompt and nothing else.
  const cases: {
    name: string;
    agentType?: SwitchableAgent;
    deltas: () => AgentDelta[];
    expect: () => void;
  }[] = [
    {
      name: "text_chunk starts an assistant text message after the prompt",
      deltas: () => [d("text_chunk", { message_id: "m1", delta: "Here " })],
      expect: () => {
        expect(messages()).toHaveLength(2);
        expect(last()).toMatchObject({ role: "assistant", mode: "text", content: "Here " });
      },
    },
    {
      name: "consecutive text_chunks append to the same message",
      deltas: () => [
        d("text_chunk", { message_id: "m1", delta: "Here " }),
        d("text_chunk", { message_id: "m1", delta: "they are." }),
      ],
      expect: () => {
        expect(messages()).toHaveLength(2);
        expect(last().content).toBe("Here they are.");
      },
    },
    {
      name: "an empty text_chunk creates nothing",
      deltas: () => [d("text_chunk", { message_id: "m1", delta: "" })],
      expect: () => expect(messages()).toHaveLength(1),
    },
    {
      name: "text_chunk skips an empty thinking marker instead of splitting the narration",
      deltas: () => [
        d("text_chunk", { message_id: "m1", delta: "One continuous " }),
        d("message_appended", {
          message: wireMessage({ id: "marker", mode: "thinking", thinking: "" }),
        }),
        d("text_chunk", { message_id: "m1", delta: "sentence." }),
      ],
      expect: () => {
        const texts = messages().filter((m) => m.role === "assistant" && m.content);
        expect(texts).toHaveLength(1);
        expect(texts[0].content).toBe("One continuous sentence.");
      },
    },
    {
      name: "text_chunk after a tool message starts a new message",
      deltas: () => [
        d("tool_call_upserted", { message_id: "t", tool_call: wireTool("tc1") }),
        d("text_chunk", { message_id: "m2", delta: "Done." }),
      ],
      expect: () => {
        expect(messages().map((m) => m.mode)).toEqual([undefined, "tool", "text"]);
        expect(last().content).toBe("Done.");
      },
    },
    {
      name: "thinking_chunks accumulate on one thinking message",
      deltas: () => [
        d("thinking_chunk", { message_id: "th", delta: "Let me " }),
        d("thinking_chunk", { message_id: "th", delta: "think." }),
      ],
      expect: () => {
        expect(messages()).toHaveLength(2);
        expect(last()).toMatchObject({ mode: "thinking", thinking: "Let me think.", content: "" });
      },
    },
    {
      name: "message_appended mirrors Rust's message, tool calls and plan converted",
      deltas: () => [
        d("message_appended", {
          message: wireMessage({
            id: "rust-1",
            content: "Plan:",
            tool_calls: [wireTool("tc1", { status: "completed", result: "a\nb" })],
            plan: [{ content: "Read the tree", status: "in_progress" }],
          }),
        }),
      ],
      expect: () => {
        expect(last()).toMatchObject({
          id: "rust-1",
          role: "assistant",
          mode: "text",
          content: "Plan:",
          thinking: "",
          fileChanges: [],
          plan: [{ id: "plan-0", description: "Read the tree", status: "in_progress" }],
        });
        expect(last().toolCalls).toEqual([
          expect.objectContaining({
            id: "tc1",
            toolName: "Bash",
            kind: "execute",
            status: "completed",
            result: "a\nb",
          }),
        ]);
      },
    },
    {
      name: "tool_call_upserted: a new call opens a tool message and is tracked in flight",
      deltas: () => [d("tool_call_upserted", { message_id: "t", tool_call: wireTool("tc1") })],
      expect: () => {
        expect(last()).toMatchObject({ role: "assistant", mode: "tool" });
        expect(last().toolCalls.map((t) => t.id)).toEqual(["tc1"]);
        expect(session().inflightToolIds).toEqual({ tc1: true });
      },
    },
    {
      name: "tool_call_upserted: consecutive calls collapse into one tool message",
      deltas: () => [
        d("tool_call_upserted", { message_id: "t1", tool_call: wireTool("tc1") }),
        d("tool_call_upserted", { message_id: "t2", tool_call: wireTool("tc2") }),
      ],
      expect: () => {
        expect(messages()).toHaveLength(2);
        expect(last().toolCalls.map((t) => t.id)).toEqual(["tc1", "tc2"]);
      },
    },
    {
      name: "tool_call_upserted: a known id is updated in place and leaves the in-flight set",
      deltas: () => [
        d("tool_call_upserted", { message_id: "t", tool_call: wireTool("tc1") }),
        d("tool_call_upserted", {
          message_id: "t",
          tool_call: wireTool("tc1", { status: "completed", result: "ok" }),
        }),
      ],
      expect: () => {
        expect(messages()).toHaveLength(2);
        expect(last().toolCalls).toHaveLength(1);
        expect(last().toolCalls[0]).toMatchObject({ status: "completed", result: "ok" });
        expect(session().inflightToolIds).toEqual({});
      },
    },
    {
      name: "tool_call_output_chunk appends live output to the call's result",
      deltas: () => [
        d("tool_call_upserted", { message_id: "t", tool_call: wireTool("tc1") }),
        d("tool_call_output_chunk", { message_id: "t", tool_call_id: "tc1", delta: "a\n" }),
        d("tool_call_output_chunk", { message_id: "t", tool_call_id: "tc1", delta: "b\n" }),
      ],
      expect: () => expect(last().toolCalls[0].result).toBe("a\nb\n"),
    },
    {
      name: "tool_call_output_chunk for an unknown call is dropped",
      deltas: () => [
        d("tool_call_output_chunk", { message_id: "t", tool_call_id: "nope", delta: "x" }),
      ],
      expect: () => expect(messages()).toHaveLength(1),
    },
    {
      name: "plan_updated hangs the plan on the trailing assistant message and the dock",
      deltas: () => [
        d("text_chunk", { message_id: "m1", delta: "Working on it." }),
        d("plan_updated", {
          plan: [
            { content: "Read", status: "completed" },
            { content: "Write", status: "pending" },
          ],
        }),
      ],
      expect: () => {
        expect(messages()).toHaveLength(2);
        expect(last().plan?.map((p) => [p.description, p.status])).toEqual([
          ["Read", "completed"],
          ["Write", "pending"],
        ]);
        expect(session().livePlan).toEqual(last().plan);
      },
    },
    {
      name: "plan_updated right after the prompt opens a message to carry it",
      deltas: () => [d("plan_updated", { plan: [{ content: "Read", status: "pending" }] })],
      expect: () => {
        expect(messages()).toHaveLength(2);
        expect(last()).toMatchObject({ role: "assistant", content: "" });
        expect(last().plan).toHaveLength(1);
      },
    },
    {
      name: "a cleared plan with nothing to hang on mints no empty bubble",
      deltas: () => [d("plan_updated", { plan: [] })],
      expect: () => {
        expect(messages()).toHaveLength(1);
        expect(session().livePlan).toEqual([]);
      },
    },
    {
      name: "model_changed moves an ACP agent's model",
      deltas: () => [d("model_changed", { model_id: "opus" })],
      expect: () => expect(session().acpCurrentModel).toBe("opus"),
    },
    {
      name: "model_changed is ignored for the native agent, whose model the UI owns",
      agentType: "atlas-agent",
      deltas: () => [d("model_changed", { model_id: "google/gemini" })],
      expect: () => expect(session().acpCurrentModel).toBeUndefined(),
    },
    {
      name: "mode_changed to a Claude permission mode moves the permission pill",
      deltas: () => [d("mode_changed", { mode_id: "acceptEdits" })],
      expect: () => {
        expect(session().acpCurrentMode).toBe("acceptEdits");
        expect(session().claudePermissionMode).toBe("acceptEdits");
      },
    },
    {
      name: "agent_disconnected flags the session for Restart and keeps the binding",
      deltas: () => [
        d("retry_status", { attempt: 1, max_attempts: 3, delay_ms: 500, last_error: "503" }),
        d("agent_disconnected", { reason: "exited with code 1" }),
      ],
      expect: () => {
        expect(session()).toMatchObject({ disconnected: true, acpSessionId: ACP });
        expect(session().retryStatus).toBeUndefined();
        expect(session().stopping).toBeUndefined();
      },
    },
    {
      name: "retry_status is shown until content resumes",
      deltas: () => [
        d("retry_status", { attempt: 2, max_attempts: 5, delay_ms: 800, last_error: "429" }),
      ],
      expect: () =>
        expect(session().retryStatus).toMatchObject({
          attempt: 2,
          maxAttempts: 5,
          delayMs: 800,
          lastError: "429",
        }),
    },
    {
      name: "a text_chunk clears retry_status — the retry worked",
      deltas: () => [
        d("retry_status", { attempt: 2, max_attempts: 5, delay_ms: 800, last_error: "429" }),
        d("text_chunk", { message_id: "m1", delta: "Back." }),
      ],
      expect: () => expect(session().retryStatus).toBeUndefined(),
    },
    {
      name: "session metadata deltas land on their fields",
      deltas: () => [
        d("title_updated", { title: "List the project files" }),
        d("available_commands", { commands: [{ name: "init" }] }),
        d("usage_updated", {
          usage: {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
          },
        }),
        d("context_usage", { used: 1200, size: 200000, cost: 0.01 }),
        d("compaction", { active: true }),
        d("rate_limits", {
          primary: { used_percent: 40, window_minutes: 300, resets_at: null },
          secondary: null,
          plan_type: "pro",
        }),
      ],
      expect: () => {
        expect(session()).toMatchObject({
          title: "List the project files",
          availableCommands: [{ name: "init" }],
          usage: { input_tokens: 10, output_tokens: 5 },
          contextUsage: { used: 1200, size: 200000, cost: 0.01 },
          compacting: true,
          rateLimits: { primary: { used_percent: 40 }, secondary: null, planType: "pro" },
        });
      },
    },
    {
      name: "elicitation_requested parks one question for the dialog",
      deltas: () => [
        d("elicitation_requested", {
          request_id: "e1",
          mode: "form",
          message: "Which branch?",
          requested_schema: { type: "object" },
        }),
      ],
      expect: () =>
        expect(session().pendingElicitation).toMatchObject({
          agentId: AGENT,
          requestId: "e1",
          mode: "form",
          message: "Which branch?",
        }),
    },
  ];

  it.each(cases)("$name", ({ agentType, deltas, expect: check }) => {
    boundTab(agentType);
    apply(...deltas());
    check();
  });

  it("ignores a delta for a session no tab is bound to", () => {
    boundTab();
    const before = useChatStore.getState().sessions;
    apply({ ...d("text_chunk", { message_id: "m", delta: "stray" }), session_id: "other" });
    expect(useChatStore.getState().sessions).toBe(before);
  });
});

describe("applyAgentDelta: status transitions", () => {
  it("running adopts the turn, clears the last turn's live plan and opens a scratch", () => {
    boundTab();
    apply(d("plan_updated", { plan: [{ content: "old", status: "completed" }] }));
    apply(d("status", { status: "running", turn_seq: 1 }));
    expect(session()).toMatchObject({ status: "running", currentTurnSeq: 1 });
    expect(session().livePlan).toBeUndefined();
    expect(session().turnScratch).toEqual({ seq: 1, tools: {} });
  });

  it("waiting keeps the session busy without starting a new turn", () => {
    boundTab();
    apply(d("status", { status: "running", turn_seq: 1 }));
    apply(d("status", { status: "waiting", turn_seq: 1 }));
    expect(session()).toMatchObject({ status: "waiting", currentTurnSeq: 1 });
  });

  it.each([
    ["idle", "completed"],
    ["error", "failed"],
  ] as const)("a bare %s sweeps live tool calls to %s", (status, swept) => {
    boundTab();
    apply(
      d("status", { status: "running", turn_seq: 1 }),
      d("tool_call_upserted", { message_id: "t", tool_call: wireTool("tc1") }),
      d("tool_call_upserted", {
        message_id: "t",
        tool_call: wireTool("tc2", { status: "pending" }),
      }),
    );
    useChatStore.setState((s) => {
      s.pendingPermissions[ACP] = [];
    });
    apply(d("status", { status, turn_seq: 1 }));
    expect(session().status).toBe(status);
    expect(last().toolCalls.map((t) => t.status)).toEqual([swept, swept]);
    expect(session().inflightToolIds).toBeUndefined();
    expect(useChatStore.getState().pendingPermissions[ACP]).toBeUndefined();
  });

  it("a terminal status for an already superseded turn is dropped", () => {
    boundTab();
    apply(d("status", { status: "running", turn_seq: 2 }));
    apply(d("status", { status: "idle", turn_seq: 1 }));
    expect(session().status).toBe("running");
  });

  it("a turn_seq of 0 (untracked) is always current", () => {
    boundTab();
    apply(d("status", { status: "running", turn_seq: 3 }));
    apply(d("status", { status: "idle", turn_seq: 0 }));
    expect(session().status).toBe("idle");
  });

  // Applied, a stale `running` would leave the composer busy for good: the
  // older turn's idle is itself stale and dropped, so nothing clears it.
  it("a running status for an already finished older turn does not reopen it", () => {
    boundTab();
    apply(
      d("status", { status: "running", turn_seq: 2 }),
      d("turn_finished", { stop_reason: "end_turn", turn_seq: 2 }),
      d("status", { status: "running", turn_seq: 1 }),
    );
    expect(session().status).toBe("idle");
  });
});

describe("applyAgentDelta: turn terminals", () => {
  it("turn_finished(end_turn) goes idle and settles every live tool call", () => {
    boundTab();
    apply(
      d("status", { status: "running", turn_seq: 1 }),
      d("tool_call_upserted", { message_id: "t", tool_call: wireTool("tc1") }),
      d("text_chunk", { message_id: "m", delta: "Done." }),
      d("turn_finished", { stop_reason: "end_turn", turn_seq: 1 }),
    );
    expect(session().status).toBe("idle");
    expect(messages()[1].toolCalls[0].status).toBe("completed");
    expect(session().inflightToolIds).toBeUndefined();
  });

  it("turn_finished stamps the turn's wall time, from the user's message, on its last message", () => {
    boundTab();
    const sentAt = Date.parse(messages()[0].timestamp);
    const now = vi.spyOn(Date, "now").mockReturnValue(sentAt + 457_000);
    try {
      apply(
        d("status", { status: "running", turn_seq: 1 }),
        d("text_chunk", { message_id: "m", delta: "Done." }),
        d("turn_finished", { stop_reason: "end_turn", turn_seq: 1 }),
      );
    } finally {
      now.mockRestore();
    }
    expect(messages()[0].role).toBe("user");
    expect(last()).toMatchObject({ role: "assistant", workedMs: 457_000 });
  });

  it("turn_finished(cancelled) marks unfinished tool calls failed", () => {
    boundTab();
    apply(
      d("tool_call_upserted", { message_id: "t", tool_call: wireTool("tc1") }),
      d("turn_finished", { stop_reason: "cancelled" }),
    );
    expect(session().status).toBe("idle");
    expect(last().toolCalls[0].status).toBe("failed");
  });

  it.each([
    ["end_turn", "(no response — the agent ended its turn without output)", "idle"],
    ["max_tokens", "(no response — stop_reason: max_tokens)", "error"],
  ] as const)(
    "an empty turn ending %s leaves a placeholder instead of a vanishing spinner",
    (stop_reason, label, status) => {
      boundTab();
      apply(d("turn_finished", { stop_reason }));
      expect(last()).toMatchObject({ role: "assistant", content: label });
      expect(session().status).toBe(status);
    },
  );

  it("an empty cancelled turn adds nothing — Stop has its own UI", () => {
    boundTab();
    apply(d("turn_finished", { stop_reason: "cancelled" }));
    expect(messages()).toHaveLength(1);
  });

  it("a truncated reply gets a notice after it, and the session reads as errored", () => {
    boundTab();
    apply(
      d("text_chunk", { message_id: "m", delta: "The files are" }),
      d("turn_finished", { stop_reason: "max_tokens" }),
    );
    expect(messages().map((m) => m.content)).toEqual([
      "list the files",
      "The files are",
      "(the reply was cut off — the model hit its output token limit)",
    ]);
    expect(session().status).toBe("error");
  });

  it("a stale turn_finished is dropped whole: no status flip, no placeholder", () => {
    boundTab();
    apply(d("status", { status: "running", turn_seq: 2 }));
    apply(d("turn_finished", { stop_reason: "end_turn", turn_seq: 1 }));
    expect(session().status).toBe("running");
    expect(messages()).toHaveLength(1);
  });

  it("turn_finished turns a trailing <next_steps> block into chips", () => {
    boundTab();
    apply(
      d("text_chunk", {
        message_id: "m",
        delta: "Listed.\n<next_steps>\n- Open the README\n- Run the tests\n</next_steps>",
      }),
      d("turn_finished", { stop_reason: "end_turn", turn_seq: 4 }),
    );
    expect(last().suggestions).toEqual({
      turnSeq: 4,
      status: "ready",
      chips: ["Open the README", "Run the tests"],
    });
  });

  it("turn_finished freezes the turn's touched files onto its last reply", () => {
    boundTab();
    apply(
      d("status", { status: "running", turn_seq: 1 }),
      d("tool_call_upserted", {
        message_id: "t",
        tool_call: wireTool("tc1", {
          tool_name: "Edit",
          kind: "edit",
          status: "completed",
          arguments: { file_path: "/tmp/project/a.ts", old_string: "a", new_string: "b\nc" },
        }),
      }),
      d("text_chunk", { message_id: "m", delta: "Edited." }),
      d("turn_finished", { stop_reason: "end_turn", turn_seq: 1 }),
    );
    expect(last().turnSummary).toMatchObject({ turnSeq: 1, repoAtTurn: false });
    expect(last().turnSummary?.files.map((f) => f.path)).toEqual(["/tmp/project/a.ts"]);
    expect(session().turnScratch).toBeUndefined();
  });

  it("turn_failed fails live tool calls, says what went wrong, and errors the session", () => {
    boundTab();
    apply(
      d("tool_call_upserted", { message_id: "t", tool_call: wireTool("tc1") }),
      d("turn_failed", { error: "HTTP 400: bad request", error_kind: "fatal" }),
    );
    expect(messages()[1].toolCalls[0].status).toBe("failed");
    expect(last()).toMatchObject({ role: "assistant", content: "Error: HTTP 400: bad request" });
    expect(session().status).toBe("error");
  });

  it("an auth turn_failed asks the composer to route to sign-in", () => {
    boundTab();
    const seen: unknown[] = [];
    const onAuth = (e: Event) => seen.push((e as CustomEvent).detail);
    window.addEventListener("atlas:auth-required", onAuth);
    try {
      apply(d("turn_failed", { error: "not logged in", error_kind: "auth" }));
    } finally {
      window.removeEventListener("atlas:auth-required", onAuth);
    }
    expect(seen).toEqual([{ sessionId: ACP, agentType: "claude-code", reason: "not logged in" }]);
  });

  it("a stale turn_failed is dropped", () => {
    boundTab();
    apply(d("status", { status: "running", turn_seq: 2 }));
    apply(d("turn_failed", { error: "old", turn_seq: 1 }));
    expect(session().status).toBe("running");
    expect(messages()).toHaveLength(1);
  });
});

describe("applyAgentDelta: history_rewound", () => {
  function twoTurns() {
    boundTab("claude-code", "first");
    apply(d("text_chunk", { message_id: "a", delta: "one" }));
    useChatStore.getState().actions.addMessage(TAB, "user", "second");
    apply(
      d("text_chunk", { message_id: "b", delta: "two" }),
      d("plan_updated", { plan: [{ content: "step", status: "pending" }] }),
    );
  }

  it("drops the last exchange and the caches derived from it", () => {
    twoTurns();
    expect(session().userMessageCount).toBe(2);
    apply(d("history_rewound", { turns: 1 }));
    expect(messages().map((m) => m.content)).toEqual(["first", "one"]);
    expect(session().userMessageCount).toBe(1);
    expect(session().livePlan).toBeUndefined();
    expect(session().firstUserContent).toBe("first");
  });

  it("rewinding every exchange empties the thread and its preview", () => {
    twoTurns();
    apply(d("history_rewound", { turns: 2 }));
    expect(messages()).toEqual([]);
    expect(session().userMessageCount).toBe(0);
    expect(session().firstUserContent).toBeUndefined();
  });

  it("a rewind past the start of the thread changes nothing", () => {
    twoTurns();
    apply(d("history_rewound", { turns: 3 }));
    expect(messages()).toHaveLength(4);
    expect(session().userMessageCount).toBe(2);
  });
});

describe("applyAgentDelta: a whole turn, in wire order", () => {
  it("running → reply → tool call with live output → reply → turn_finished → idle", () => {
    boundTab();
    apply(
      d("status", { status: "running", turn_seq: 1 }),
      d("message_appended", {
        message: wireMessage({ id: "r1", mode: "thinking", thinking: "I should look." }),
      }),
      d("message_appended", { message: wireMessage({ id: "r2", content: "Let me " }) }),
      d("text_chunk", { message_id: "r2", delta: "look." }),
      d("tool_call_upserted", {
        message_id: "r3",
        tool_call: wireTool("ls", { status: "pending" }),
      }),
    );
    expect(session().status).toBe("running");
    expect(session().inflightToolIds).toEqual({ ls: true });

    apply(
      d("tool_call_upserted", { message_id: "r3", tool_call: wireTool("ls") }),
      d("tool_call_output_chunk", { message_id: "r3", tool_call_id: "ls", delta: "a.ts\n" }),
      d("tool_call_output_chunk", { message_id: "r3", tool_call_id: "ls", delta: "b.ts\n" }),
      d("tool_call_upserted", {
        message_id: "r3",
        tool_call: wireTool("ls", { status: "completed", result: "a.ts\nb.ts\n" }),
      }),
      d("message_appended", { message: wireMessage({ id: "r4", content: "Two files." }) }),
      d("usage_updated", {
        usage: {
          input_tokens: 90,
          output_tokens: 12,
          cache_creation_tokens: 0,
          cache_read_tokens: 0,
        },
      }),
      d("turn_finished", { stop_reason: "end_turn", turn_seq: 1 }),
      d("status", { status: "idle", turn_seq: 1 }),
    );

    expect(session().status).toBe("idle");
    expect(session().inflightToolIds).toBeUndefined();
    expect(messages().map((m) => [m.role, m.mode ?? null, m.content, m.toolCalls.length])).toEqual([
      ["user", null, "list the files", 0],
      ["assistant", "thinking", "", 0],
      ["assistant", "text", "Let me look.", 0],
      ["assistant", "tool", "", 1],
      ["assistant", "text", "Two files.", 0],
    ]);
    expect(messages()[1].thinking).toBe("I should look.");
    expect(messages()[3].toolCalls[0]).toMatchObject({
      id: "ls",
      status: "completed",
      result: "a.ts\nb.ts\n",
    });
    // The full snapshot replaced the streamed result rather than doubling it.
    expect(messages()[3].toolCalls[0].result).toBe("a.ts\nb.ts\n");
  });

  // `startedAt` is the only field on a tool call the wire does not carry — the
  // session-delta wire is frozen and has no start time, so the store stamps
  // one. The live elapsed figure on a running block is read from it, which
  // makes "stamped once, never restarted" the invariant worth pinning.
  it("stamps a tool call's start on first sight and keeps it across updates", () => {
    boundTab();
    const before = Date.now();
    apply(d("tool_call_upserted", { message_id: "t", tool_call: wireTool("tc1") }));
    const startedAt = last().toolCalls[0].startedAt;
    expect(startedAt).toBeGreaterThanOrEqual(before);

    // The completion arrives as an upsert for the SAME id, and `toChatToolCall`
    // mints a record without the field. If that overwrote the stamp the clock
    // would reset on every status change the agent reported.
    apply(
      d("tool_call_upserted", {
        message_id: "t",
        tool_call: wireTool("tc1", { status: "completed", result: "ok" }),
      }),
    );
    expect(last().toolCalls[0]).toMatchObject({ status: "completed", startedAt });
  });
});
