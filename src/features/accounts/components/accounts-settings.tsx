import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { FolderOpen, Plus, Trash2 } from "lucide-react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Button } from "@/ui/button";
import { Input } from "@/ui/input";
import { Hint } from "@/ui/tooltip";
import { tildePath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { listenCatalogChanged } from "@/features/chat/lib/agents-api";
import { ACCOUNT_BASES, accounts, type Account } from "../lib/accounts-api";

/**
 * Settings → Accounts.
 *
 * Lists the extra logins installed as agent entries and adds new ones. Adding
 * an account only creates its entry and private profile home; signing in
 * happens in the agent's own flow the first time a chat starts on it.
 */
export function AccountsSettings() {
  const [list, setList] = useState<Account[] | null>(null);
  const [baseId, setBaseId] = useState(ACCOUNT_BASES[0].id);
  const [label, setLabel] = useState("");
  const [adding, setAdding] = useState(false);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);

  const reload = useCallback(() => {
    accounts
      .list()
      .then(setList)
      .catch((e) => toast.error(`Could not load accounts: ${String(e)}`));
  }, []);

  // The installed map also changes from the Agents marketplace, so the list
  // follows every catalog change rather than only its own writes.
  useEffect(() => {
    reload();
    const pending = listenCatalogChanged(() => reload());
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, [reload]);

  const add = async () => {
    setAdding(true);
    try {
      const account = await accounts.create(baseId, label);
      setLabel("");
      toast.success(
        `Added ${account.displayName ?? account.label}. Start a chat with it to sign in.`,
      );
      reload();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setAdding(false);
    }
  };

  const remove = async (account: Account) => {
    setConfirmingId(null);
    try {
      await accounts.remove(account.id);
      reload();
    } catch (e) {
      toast.error(`Could not remove ${account.label}: ${String(e)}`);
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <div>
        <p className="text-sm font-medium text-foreground">Accounts</p>
        <p className="mt-0.5 text-2xs text-muted-foreground">
          Run Claude Code or Codex under more than one login. Each account is its own agent in the
          picker, with its own settings and sign-in kept in a private folder. Your existing login
          stays the default. Quotatlas never reads the credentials.
        </p>
      </div>

      <div className="flex flex-col gap-2">
        <div className="flex gap-1">
          {ACCOUNT_BASES.map((base) => (
            <Button
              key={base.id}
              size="sm"
              variant={base.id === baseId ? "secondary" : "ghost"}
              aria-pressed={base.id === baseId}
              onClick={() => setBaseId(base.id)}
            >
              {base.name}
            </Button>
          ))}
        </div>
        <div className="flex gap-2">
          <Input
            size="sm"
            value={label}
            placeholder="Account name, e.g. work"
            aria-label="Account name"
            maxLength={40}
            onChange={(e) => setLabel(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && label.trim() && !adding) void add();
            }}
          />
          <Button size="sm" disabled={!label.trim() || adding} onClick={() => void add()}>
            <Plus size={12} />
            Add account
          </Button>
        </div>
      </div>

      <div className="flex flex-col gap-1">
        {list === null ? null : list.length === 0 ? (
          <p className="text-2xs text-muted-foreground">
            No extra accounts yet. Add one above, then pick it when you start a chat.
          </p>
        ) : (
          list.map((account) => (
            <div
              key={account.id}
              className="flex items-center justify-between gap-3 rounded border border-border-subtle px-2 py-1.5"
            >
              <div className="min-w-0">
                <p className="truncate text-xs font-medium text-foreground">
                  {account.displayName ?? `${account.baseAgentId} · ${account.label}`}
                </p>
                <p className="truncate text-2xs text-muted-foreground">
                  {tildePath(account.profileHome)}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <Hint label="Show profile folder">
                  <Button
                    size="xs"
                    variant="ghost"
                    aria-label="Show profile folder"
                    onClick={() => void revealItemInDir(account.profileHome)}
                  >
                    <FolderOpen size={12} />
                  </Button>
                </Hint>
                <Button
                  size="xs"
                  variant={confirmingId === account.id ? "destructive" : "ghost"}
                  className={cn(confirmingId !== account.id && "text-muted-foreground")}
                  onClick={() =>
                    confirmingId === account.id ? void remove(account) : setConfirmingId(account.id)
                  }
                  onBlur={() => setConfirmingId((id) => (id === account.id ? null : id))}
                >
                  <Trash2 size={12} />
                  {confirmingId === account.id ? "Remove" : null}
                </Button>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
