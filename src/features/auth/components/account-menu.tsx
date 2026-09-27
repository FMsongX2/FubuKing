// Modified by Quotatlas from upstream Atlas (Apache-2.0).
import type { ReactElement } from "react";
import { Menu as DropdownMenu } from "@base-ui/react/menu";
import { Keyboard, LayoutTemplate, LogIn, LogOut, Palette, Settings, Zap } from "lucide-react";
import { toast } from "sonner";

import { KbdCombo } from "@/ui/kbd";
import { openSettingsSection } from "@/features/settings/lib/open-settings";
import type { SettingsSection } from "@/features/settings/stores/settings-nav-store";
import { type AccountUser, type SignedIn, type SignedOut } from "../lib/auth-api";
import { useAuthStore } from "../stores/auth-store";
import { AccountAvatar } from "./account-avatar";
import { HOSTED_SERVICES_ENABLED } from "@/lib/quotatlas";

/**
 * The account menu's destinations.
 *
 * `label` is what the user is looking for and `section` is where it lives —
 * "Themes" is the Appearance section, not one of its own.
 *
 * `shortcut` is set only where Atlas really binds one, so a hint here always
 * survives being pressed.
 */
const ITEMS: Array<{
  label: string;
  icon: typeof Settings;
  section: SettingsSection;
  shortcut?: string;
}> = [
  { label: "Settings", icon: Settings, section: "general", shortcut: "⌘," },
  { label: "Keybindings", icon: Keyboard, section: "keybindings" },
  { label: "Themes", icon: Palette, section: "appearance" },
  { label: "Skills", icon: Zap, section: "skills" },
  { label: "Layouts", icon: LayoutTemplate, section: "layouts" },
];

// Same shape as the composer's "+" menu, so every Atlas dropdown reads alike.
// `z-popover` (decision 28) is the layer every menu sits on: the content is
// portalled to `body`, so it shares a stacking context with every dialog and
// overlay in the app, and a menu has to sit above a dialog rather than under it.
const CONTENT_CLASS =
  "min-w-[228px] max-w-[300px] rounded-md border border-[var(--border)] " +
  "bg-[var(--card)] shadow-md py-1";

/**
 * Shown when the background revocation did not land — offline, or an Atlas
 * outage.
 *
 * States the residual risk instead of implying a clean revocation, because the
 * session is a 7-day rolling credential: "may stay active until it expires" is
 * a real window, and only the user can judge whether it matters on the machine
 * they are walking away from.
 */
const RESIDUAL_SESSION =
  "Signed out on this device. Your Atlas session may stay active on the server until it expires.";

// `cursor-pointer`, not the `cursor-default` a native menu would use: every row
// carrying this class does something when clicked, and the pointer says so.
// It is deliberately on the class the *actionable* rows share — the identity
// header is a `Label` and keeps the arrow, so the cursor distinguishes what you
// can act on from what is only being reported.
const ITEM_CLASS =
  "flex items-center gap-2 px-3 h-[26px] text-xs cursor-pointer outline-none " +
  "text-[var(--secondary-foreground)] data-[highlighted]:bg-[var(--atlas-element-hover)] " +
  "data-[highlighted]:text-[var(--foreground)]";

// Hoisted for the same reason as the two above: this menu now has three
// separators, and a rule one of them disagreed with would be visible.
const SEPARATOR_CLASS = "my-1 h-px bg-[var(--border)]";

/**
 * The menu behind the title bar's avatar — who you are, the parts of Settings
 * people actually reach for, and the way out.
 *
 * Wraps the account button rather than replacing it, so signed out the button
 * keeps its plain click-to-sign-in behaviour and this component is simply not
 * rendered. Escape, click-away, arrow keys and type-ahead come from Radix, the
 * same primitive every other Atlas menu uses.
 */
export function AccountMenu({
  account,
  children,
}: {
  /**
   * The whole signed-in snapshot rather than its pieces, so the menu cannot be
   * handed an identity and a set of organisations that came from different
   * moments.
   *
   * `account.user` is `null` in the narrow window between holding a credential
   * and the first successful profile fetch (see `SignedIn.user`). The menu still
   * opens, and Sign Out still works — neither depends on identity, and a
   * credential we cannot put a name to is still one the user must be able to
   * disconnect. There is simply no header to render, and inventing a placeholder
   * name would be worse than the gap it fills.
   */
  account: SignedIn | SignedOut;
  children: ReactElement;
}) {
  const { signOut, beginSignIn } = useAuthStore.use.actions();
  const signedIn = account.status === "signed-in";
  const user = signedIn ? account.user : null;

  // Not awaited by anything on screen: the title bar has already flipped by the
  // time this promise settles, off the state event Rust emits before it touches
  // the network. All that arrives here is whether a caveat is owed.
  const onSignOut = async () => {
    if (!(await signOut())) toast.warning(RESIDUAL_SESSION);
  };

  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger render={children} />
      <DropdownMenu.Portal>
        <DropdownMenu.Positioner className="z-popover" align="end" sideOffset={6}>
          <DropdownMenu.Popup className={CONTENT_CLASS}>
            {/* The account zone is always first, whichever state we are in: the
                identity when there is one, the way to get one when there is not.
                Keeping it in the same place means the destinations below never
                move under the pointer as the user signs in or out. */}
            {user && <Header user={user} />}
            {!signedIn && HOSTED_SERVICES_ENABLED && (
              <>
                <DropdownMenu.Item onClick={() => void beginSignIn()} className={ITEM_CLASS}>
                  <LogIn size={13} className="shrink-0 text-[var(--muted-foreground)]" />
                  <span className="flex-1 text-left">Sign In</span>
                </DropdownMenu.Item>
                <DropdownMenu.Separator className={SEPARATOR_CLASS} />
              </>
            )}
            {ITEMS.map((item) => (
              <DropdownMenu.Item
                key={item.section}
                onClick={() => openSettingsSection(item.section)}
                className={ITEM_CLASS}
              >
                <item.icon size={13} className="shrink-0 text-[var(--muted-foreground)]" />
                <span className="flex-1 text-left">{item.label}</span>
                {item.shortcut && <KbdCombo combo={item.shortcut} />}
              </DropdownMenu.Item>
            ))}
            {/* Separated from the destinations above: everything else in this
                menu navigates, and this one ends the session. Rendered whenever a
                credential is held, with or without a profile — one we cannot put
                a name to is still one the user must be able to disconnect. */}
            {signedIn && (
              <>
                <DropdownMenu.Separator className={SEPARATOR_CLASS} />
                <DropdownMenu.Item onClick={() => void onSignOut()} className={ITEM_CLASS}>
                  <LogOut size={13} className="shrink-0 text-[var(--muted-foreground)]" />
                  <span className="flex-1 text-left">Sign Out</span>
                </DropdownMenu.Item>
              </>
            )}
          </DropdownMenu.Popup>
        </DropdownMenu.Positioner>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** Avatar, name and email — the "who am I signed in as" answer. */
function Header({ user }: { user: AccountUser }) {
  // A name is not guaranteed: some providers give us only an address. Falling
  // back to the email keeps the primary line filled rather than showing a blank
  // row above the address, and the second line is dropped when it would just
  // repeat the first.
  const name = user.name.trim();
  const email = user.email.trim();
  const primary = name || email;

  return (
    <>
      {/* `GroupLabel`, not a bare div: inside `role="menu"` an unlabelled block
          is announced as loose text between the items. This is the menu's
          title. Base UI reads its group from context and throws when there is
          none — where Radix's `Label` stood alone — so the `Group` wrapper is
          required, not decorative. */}
      <DropdownMenu.Group>
        <DropdownMenu.GroupLabel className="flex items-center gap-2.5 px-3 py-1.5">
          <AccountAvatar user={user} size={28} />
          {/* `flex-1 min-w-0` against the content's max width is what makes a
            long address truncate rather than stretch the whole menu. */}
          <div className="flex-1 min-w-0">
            <div className="truncate text-sm font-medium text-[var(--foreground)]">{primary}</div>
            {email && email !== primary && (
              <div className="truncate text-xs text-[var(--muted-foreground)]">{email}</div>
            )}
          </div>
        </DropdownMenu.GroupLabel>
      </DropdownMenu.Group>
      <DropdownMenu.Separator className={SEPARATOR_CLASS} />
    </>
  );
}
