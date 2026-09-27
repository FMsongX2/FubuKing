/**
 * A recorded tool call as one sentence — "Read flow.tsx", "Ran bun test",
 * "Searched anchorId in features/chat".
 *
 * The live transcript already reads tool calls this way (`markerFor` in
 * `chat/lib/turn-rows.ts`), and the Timeline used to read them as a table of
 * `toolName` and `paths[0]` in monospace. Two vocabularies for the same event
 * meant the same call was recognisable in one surface and not the other, which
 * matters most exactly where the two meet: a comment made on a call in the
 * Timeline is read back beside that call in the chat.
 *
 * # Why this is not just `markerFor`
 *
 * `markerFor` classifies from the WIRE: an ACP `kind` plus the agent's own tool
 * name. The record has neither — `tools.rs` derives a **canonical** name
 * (`Read`, `Edit`, `Write`, `Bash`, `Search`, `Fetch`, `Delete`, `Move`,
 * `Think`, `Task`, `Other`) and stores that, because the wire has no canonical
 * name to store. So the classification here is a table over those eleven
 * values, and the chat's own helpers do the rest of the work:
 *
 * * `parseShellCommand` reduces a command to its real action, so a recorded
 *   `cat src/main.rs` reads as "Read main.rs" here exactly as it does live.
 * * `getFilePathFromInput` / `isFileCreated` answer which file, and whether the
 *   call created it.
 * * `toolIconFor` picks the glyph for an `Other` call, which is where the MCP
 *   name sniff still applies.
 *
 * Pure, and cheap enough for a row: one `JSON.parse` of the recorded arguments,
 * memoised by the caller per entry.
 */

import { bashCommandOf, isBashToolCall } from "@/features/chat/lib/tool-calls";
import { parseShellCommand } from "@/features/chat/lib/parse-shell-command";
import {
  classifyToolFileKind,
  getFilePathFromInput,
  isFileCreated,
} from "@/features/chat/lib/tool-files";
import { shortPath, toolIconFor, type MarkerTool } from "@/features/chat/lib/turn-rows";

import type { TimelineEntry } from "../types";

export interface ToolLine {
  /** Which glyph leads the line. */
  tool: MarkerTool;
  /** "Read", "Edited", "Ran" — what happened. */
  verb: string;
  /** The command, path or pattern it happened to. May be empty. */
  detail: string;
  /** Is `detail` a file? Only then does it get the dotted-underline treatment,
   *  matching the transcript: a command is not a link. */
  fileDetail: boolean;
}

/**
 * The glyph and past-tense verb of each canonical name (`ToolName` in
 * `crates/atlas-checkpoint/src/tools.rs`). `Bash` is absent: a command is read
 * for what it did rather than labelled "Ran" outright. `Other` is absent too —
 * it carries no meaning to map, so the agent's own title speaks for it.
 */
const CANONICAL: Record<string, { tool: MarkerTool; verb: string }> = {
  Read: { tool: "read", verb: "Read" },
  Edit: { tool: "edit", verb: "Edited" },
  Write: { tool: "edit", verb: "Created" },
  Search: { tool: "search", verb: "Searched" },
  Fetch: { tool: "fetch", verb: "Fetched" },
  Delete: { tool: "delete", verb: "Deleted" },
  Move: { tool: "move", verb: "Moved" },
  // A delegated sub-agent is a different KIND of work from a file read, and the
  // transcript gives it the brain for that reason.
  Think: { tool: "think", verb: "Thought" },
  Task: { tool: "think", verb: "Delegated" },
};

/**
 * A tool name the table above knows, for a record that does not hold a
 * canonical one.
 *
 * Everything `tools.rs` writes today IS canonical, so this only catches two
 * cases: a Session imported from an agent's own JSONL before canonicalisation
 * existed, and a name the enum has since gained. The sniff is deliberately
 * narrow — a wrong verb is worse than the generic one, which is why substrings
 * that appear inside unrelated words are not in it.
 */
const SNIFF: [RegExp, string][] = [
  [/grep|search|glob|find_/, "Search"],
  [/read|\bcat\b|view_|open_file/, "Read"],
  [/fetch|http|web_/, "Fetch"],
  [/delete|remove/, "Delete"],
  [/rename|move/, "Move"],
  [/task|agent|delegate/, "Task"],
];

function canonicalKey(name: string): string | undefined {
  if (CANONICAL[name]) return name;
  // The edit family has its own table, shared with the live transcript, and it
  // wins: `create_file` must not be sniffed as anything else.
  if (classifyToolFileKind(null, name) === "edit") return "Edit";
  const lower = name.toLowerCase();
  for (const [pattern, key] of SNIFF) {
    if (pattern.test(lower)) return key;
  }
  return undefined;
}

/** The recorded arguments as an object, or `{}` for anything unparseable —
 *  a guess about a malformed payload is worse than saying less about it. */
function argsOf(entry: TimelineEntry): Record<string, unknown> {
  if (!entry.arguments) return {};
  try {
    const parsed: unknown = JSON.parse(entry.arguments);
    return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

export function toolLine(entry: TimelineEntry): ToolLine {
  const name = entry.toolName ?? "Other";
  const args = argsOf(entry);
  const title = entry.toolTitle && entry.toolTitle !== name ? clean(entry.toolTitle) : "";
  // The arguments name the file when they were recorded; `paths` is what
  // `tools.rs` resolved, and is there even for a call whose arguments spilled
  // to a blob.
  const path = getFilePathFromInput(args) ?? entry.paths[0] ?? null;

  if (isBashToolCall({ kind: null, toolName: name })) {
    // The recorded title IS the command for agents that send it as one, which
    // is what keeps a command-line readable when its arguments spilled.
    const command = bashCommandOf(args) || title;
    const parsed = parseShellCommand(command);
    if (parsed.kind === "read") {
      return { tool: "read", verb: "Read", detail: shortPath(parsed.path), fileDetail: true };
    }
    if (parsed.kind === "list") {
      return {
        tool: "list",
        verb: "Listed",
        detail: parsed.path ? shortPath(parsed.path) : "",
        fileDetail: true,
      };
    }
    if (parsed.kind === "search") {
      // The pattern verbatim (a regex may contain slashes `shortPath` would
      // eat); only the path is shortened.
      const where = parsed.path ? ` in ${shortPath(parsed.path)}` : "";
      return {
        tool: "search",
        verb: "Searched",
        detail: clean(`${parsed.query ?? ""}${where}`),
        fileDetail: false,
      };
    }
    return { tool: "run", verb: "Ran", detail: clean(command), fileDetail: false };
  }

  const known = CANONICAL[canonicalKey(name) ?? ""];
  if (known) {
    if (known.tool === "edit") {
      return {
        tool: "edit",
        // `Write` is usually a creation and `Edit` usually is not, but both
        // tools do both — the arguments are what actually say so.
        verb: isFileCreated(name, args) ? "Created" : "Edited",
        detail: path ? shortPath(path) : title,
        fileDetail: path !== null,
      };
    }
    if (known.tool === "search") {
      const pattern = typeof args.pattern === "string" ? args.pattern : (args.query as string);
      const where = path ? ` in ${shortPath(path)}` : "";
      const detail = typeof pattern === "string" ? clean(`${pattern}${where}`) : title;
      return { tool: "search", verb: known.verb, detail, fileDetail: false };
    }
    return {
      tool: known.tool,
      verb: known.verb,
      detail: path ? shortPath(path) : title,
      fileDetail: path !== null,
    };
  }

  // `Other`: an MCP call, or a tool `tools.rs` had no bucket for. Its recorded
  // title is the agent's own one-line description, which reads better than the
  // word "Other" followed by nothing — and the name sniff still picks a glyph.
  const tool = toolIconFor(null, title || name);
  if (title) return { tool, verb: title, detail: "", fileDetail: false };
  return {
    tool: path && tool === "tool" ? "file" : tool,
    verb: "Used a tool",
    detail: path ? shortPath(path) : "",
    fileDetail: path !== null,
  };
}

function clean(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}
