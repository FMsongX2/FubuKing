import { describe, expect, it } from "vitest";
import type { AccountQuota } from "./quota-api";
import {
  formatCountdown,
  formatPlan,
  quotaLevel,
  remainingPercent,
  tightestWindow,
  worstLevel,
} from "./quota-format";

function account(
  windows: Array<[string, number]>,
  status: AccountQuota["status"] = "ok",
): AccountQuota {
  return {
    accountId: "codex-acp@work",
    provider: "codex",
    label: "work",
    displayName: "Codex · work",
    isDefault: false,
    plan: "pro",
    status,
    message: null,
    windows: windows.map(([label, usedPercent]) => ({
      id: label.toLowerCase(),
      label,
      usedPercent,
      windowMinutes: null,
      resetsAt: null,
    })),
    updatedAt: 0,
  };
}

describe("quota formatting", () => {
  it("turns usage into a clamped whole percent left", () => {
    expect(remainingPercent({ usedPercent: 11 })).toBe(89);
    expect(remainingPercent({ usedPercent: 120 })).toBe(0);
    expect(remainingPercent({ usedPercent: -3 })).toBe(100);
  });

  it("uses Quotio's thresholds for the colour levels", () => {
    expect(quotaLevel(51)).toBe("good");
    expect(quotaLevel(50)).toBe("low");
    expect(quotaLevel(21)).toBe("low");
    expect(quotaLevel(20)).toBe("critical");
  });

  it("counts down in the two largest units", () => {
    const now = 1_000_000_000_000;
    const at = (minutes: number) => now / 1000 + minutes * 60;
    expect(formatCountdown(at(2 * 1440 + 17 * 60 + 5), now)).toBe("2d 17h");
    expect(formatCountdown(at(4 * 60 + 59), now)).toBe("4h 59m");
    expect(formatCountdown(at(12), now)).toBe("12m");
    expect(formatCountdown(at(-1), now)).toBe("now");
    expect(formatCountdown(null, now)).toBeNull();
  });

  it("capitalises plans", () => {
    expect(formatPlan("pro")).toBe("Pro");
    expect(formatPlan(null)).toBeNull();
  });

  it("finds the window that runs out first and the worst level", () => {
    const a = account([
      ["Session", 9],
      ["Weekly", 71],
    ]);
    expect(tightestWindow(a)?.label).toBe("Weekly");
    expect(worstLevel([a, account([["Weekly", 95]])])).toBe("critical");
    expect(worstLevel([account([["Weekly", 95]], "pending")])).toBeNull();
  });
});
