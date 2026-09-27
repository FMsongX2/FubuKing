// Scenario registry. Add a file next to this one and list it here; it is then
// reachable at `localhost:1420/?scenario=<name>`.

import type { Scenario } from "../types";
import { commsIncomingMessage } from "../fixtures/comms";
import {
  requestPermission,
  requestPermissionLongArgs,
  requestPermissionPlan,
  requestPermissionQuestion,
  requestPermissionQuestionMulti,
} from "../fake-agent";
import {
  chatComments,
  chatCommentsError,
  chatCommentsMany,
  chatCommentsOrphan,
  chatCommentsRepliesOnly,
  chatCommentsTools,
} from "./chat-comments";
import { chatLongPrompt } from "./chat-long-prompt";
import { chatMarkdown } from "./chat-markdown";
import { chatTools } from "./chat-tools";
import { designSystem } from "./design-system";
import { gitConflict } from "./git-conflict";
import { knowledge } from "./knowledge";

/** Every variant `permission-modal.tsx` renders, callable from any chat
 *  scenario once a session is bound: a plain command, a long/multi-line one,
 *  the ExitPlanMode review, and Claude's AskUserQuestion bridge (single- and
 *  multi-question). See `fake-agent.ts` for how each raises a real
 *  `permission_request` delta. */
const permissionActions = {
  requestPermission,
  requestPermissionLongArgs,
  requestPermissionPlan,
  requestPermissionQuestion,
  requestPermissionQuestionMulti,
};

const all: Scenario[] = [
  {
    name: "default",
    description: "Every surface, populated. The one to review a theme against.",
    // The only thing the default scenario cannot show by sitting still: a
    // message arriving while you are looking at something else.
    actions: { commsIncomingMessage, ...permissionActions },
  },
  chatTools,
  chatComments,
  chatCommentsOrphan,
  chatCommentsMany,
  chatCommentsRepliesOnly,
  chatCommentsTools,
  chatCommentsError,
  chatLongPrompt,
  chatMarkdown,
  gitConflict,
  knowledge,
  designSystem,
];

export const scenarios: Record<string, Scenario> = Object.fromEntries(all.map((s) => [s.name, s]));
