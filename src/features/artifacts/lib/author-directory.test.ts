import { describe, expect, it } from "vitest";

import type { OrgMember } from "@/features/auth/lib/auth-api";

import { authorOf, firstName, type AuthorDirectory } from "./author-directory";

function member(userId: string, name: string): OrgMember {
  // `id` is deliberately NOT `userId`: a row's `authorId` is the human, and a
  // directory keyed by the membership id would never match one.
  return {
    id: `mem_${userId}`,
    userId,
    name,
    email: `${userId}@example.invalid`,
    role: "member",
    createdAt: null,
    avatarPath: null,
  } as OrgMember;
}

function directory(currentUserId: string | null, ...members: OrgMember[]): AuthorDirectory {
  return { byId: new Map(members.map((m) => [m.userId, m])), currentUserId };
}

describe("firstName", () => {
  it("keeps the first word of a full name", () => {
    expect(firstName("Ada Lovelace")).toBe("Ada");
    expect(firstName("Ada")).toBe("Ada");
  });

  it("stops a name entered in caps from shouting", () => {
    expect(firstName("AZRAF AL MONZIM")).toBe("Azraf");
    expect(firstName("GRACE")).toBe("Grace");
  });

  it("leaves a name alone if its owner mixed the case themselves", () => {
    // Blanket title-casing corrects people's names for them. Any lowercase at
    // all means the casing was a choice, so it stands.
    expect(firstName("McDonald Smith")).toBe("McDonald");
    expect(firstName("danah boyd")).toBe("danah");
    expect(firstName("van Gogh")).toBe("van");
    expect(firstName("O'Brien")).toBe("O'Brien");
  });

  it("does not mangle a name with no case at all", () => {
    // Han, Arabic, digits: `toUpperCase` is a no-op, so the shouting test must
    // not fire on them.
    expect(firstName("王小明")).toBe("王小明");
    expect(firstName("محمد")).toBe("محمد");
  });

  it("survives padding and double spaces", () => {
    expect(firstName("  Grace   Hopper ")).toBe("Grace");
  });

  it("returns the original rather than an empty string", () => {
    // A blank label is worse than an odd one: the row would lose its column.
    expect(firstName("   ")).toBe("   ");
  });
});

describe("authorOf", () => {
  it("calls a local Session yours without a lookup", () => {
    // `authorId` is null on a row captured here, and a local Session is this
    // account's by construction — there is nothing to resolve.
    const author = authorOf(null, directory("user_ada"));
    expect(author.label).toBe("You");
    expect(author.isSelf).toBe(true);
  });

  it("calls your own synced Session yours too", () => {
    // The same work pushed and read back carries your id, and a row that said
    // "Ada" beside your own face would read as someone else's.
    const ada = member("user_ada", "Ada Lovelace");
    const author = authorOf("user_ada", directory("user_ada", ada));
    expect(author.label).toBe("You");
    expect(author.avatar?.id).toBe("user_ada");
  });

  it("gives a colleague their first name and their face", () => {
    const grace = member("user_grace", "Grace Hopper");
    const author = authorOf("user_grace", directory("user_ada", grace));
    expect(author.label).toBe("Grace");
    expect(author.avatar?.name).toBe("Grace Hopper");
    // The avatar's colour is derived from the id it is given, so it must be the
    // human's — not the membership's, which differs per Organisation.
    expect(author.avatar?.id).toBe("user_grace");
    expect(author.isSelf).toBe(false);
  });

  it("names an unknown author without printing their id", () => {
    // A departed colleague still owns their Sessions. An opaque key in the
    // byline is noise, and it is what the row used to show.
    const author = authorOf("user_gone", directory("user_ada"));
    expect(author.label).toBe("A member");
    expect(author.avatar).toBeNull();
  });

  it("does not claim a signed-out reader owns anything", () => {
    // With no account there is no "you" to be — every row is someone's.
    const author = authorOf("user_grace", directory(null));
    expect(author.isSelf).toBe(false);
    expect(author.label).toBe("A member");
  });

  it("still says You for a local row when signed out", () => {
    // It is on this disk; nobody else recorded it.
    expect(authorOf(null, directory(null)).label).toBe("You");
  });
});
