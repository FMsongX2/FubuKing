import { ArrowLeftRight, Download } from "lucide-react";
import { cn } from "@/lib/utils";
import { openSettingsSection } from "@/features/settings/lib/open-settings";
import { cycleChatAgent } from "../lib/switch-agent";
import { COMPOSER_STRIP, COMPOSER_STRIP_ACTION } from "./composer-strip";

/**
 * FubuKing: the native agent's composer notice. Upstream Atlas runs that
 * agent on its hosted gateway, which FubuKing does not use (ADR Q-0001), so
 * instead of a sign-in that cannot succeed the strip says so and offers the two
 * ways forward: switch this chat to another agent, or install one.
 */
export function HostedAgentBar({ tabId }: { tabId: string }) {
  return (
    <div
      data-testid="hosted-agent-bar"
      className={COMPOSER_STRIP}
      title="The built-in agent needs Atlas's hosted gateway, which FubuKing does not use"
    >
      <span className="min-w-0 truncate text-muted-foreground">
        The built-in agent is not available in FubuKing. Use Claude Code or Codex.
      </span>
      <div className="flex shrink-0 items-center gap-1">
        <button
          type="button"
          onClick={() => cycleChatAgent(tabId)}
          className={cn(COMPOSER_STRIP_ACTION, "cursor-pointer")}
        >
          <ArrowLeftRight size={11} />
          Switch agent
        </button>
        <button
          type="button"
          onClick={() => openSettingsSection("agents")}
          className={cn(COMPOSER_STRIP_ACTION, "cursor-pointer")}
        >
          <Download size={11} />
          Install
        </button>
      </div>
    </div>
  );
}
