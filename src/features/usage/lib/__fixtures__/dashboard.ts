import type { DailyBucket, SessionRow, UsageDashboard } from "../../types";

// Deterministic PRNG so screenshots are reproducible.
let seed = 42;
const rnd = () => ((seed = (seed * 16807) % 2147483647) - 1) / 2147483646;

const pad = (n: number) => String(n).padStart(2, "0");
const key = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

const PROJECTS = [
  ["/Users/adib/Desktop/atlas", "atlas"],
  ["/Users/adib/Desktop/site", "site"],
  ["/Users/adib/Desktop/erp-crm-design", "erp-crm-design"],
  ["/Users/adib/work/ledger", "ledger"],
] as const;
const AGENTS = ["claude-code", "codex", "atlas-agent"] as const;
const MODELS: Record<string, string[]> = {
  "claude-code": ["claude-opus-5", "claude-sonnet-5"],
  codex: ["gpt-5.5-codex"],
  "atlas-agent": ["gpt-5.5-codex", "claude-opus-5"],
};
const PRICE: Record<string, [number, number, number, number]> = {
  "claude-opus-5": [15, 75, 1.5, 18.75],
  "claude-sonnet-5": [3, 15, 0.3, 3.75],
  "gpt-5.5-codex": [1.25, 10, 0.125, 0],
};
const cost = (m: string, i: number, o: number, cr: number, cw: number) => {
  const p = PRICE[m] ?? [0, 0, 0, 0];
  return (i * p[0] + o * p[1] + cr * p[2] + cw * p[3]) / 1e6;
};

export function fixture(days = 75): UsageDashboard {
  const daily: DailyBucket[] = [];
  const sessions: SessionRow[] = [];
  const today = new Date();
  let sid = 0;
  for (let d = days - 1; d >= 0; d--) {
    const day = new Date(today);
    day.setDate(day.getDate() - d);
    const k = key(day);
    const dow = day.getDay();
    const weekend = dow === 0 || dow === 6;
    for (const [path] of PROJECTS) {
      for (const agent of AGENTS) {
        if (rnd() < (weekend ? 0.75 : 0.45)) continue;
        const model = MODELS[agent][Math.floor(rnd() * MODELS[agent].length)];
        const turns = 1 + Math.floor(rnd() * 12);
        const input = Math.round(turns * (2_000 + rnd() * 9_000));
        const output = Math.round(turns * (400 + rnd() * 2_500));
        const cacheRead = agent === "codex" ? 0 : Math.round(input * (3 + rnd() * 9));
        const cacheWrite = agent === "codex" ? 0 : Math.round(input * (0.4 + rnd() * 0.8));
        const reasoning = agent === "atlas-agent" ? Math.round(output * 0.35) : 0;
        const c = cost(model, input, output, cacheRead, cacheWrite);
        const nSessions = 1 + (rnd() < 0.3 ? 1 : 0);
        daily.push({
          date: k,
          projectPath: path,
          agent,
          model,
          input,
          output,
          cacheRead,
          cacheWrite,
          reasoning,
          cost: c,
          messages: turns * 2,
          sessions: nSessions,
        });
        for (let s = 0; s < nSessions; s++) {
          const f = 1 / nSessions;
          const start = day.getTime() + Math.floor(rnd() * 8) * 3_600_000 + 9 * 3_600_000;
          sessions.push({
            sessionId: `sess-${++sid}-${Math.floor(rnd() * 1e6).toString(16)}`,
            projectPath: path,
            agent,
            model,
            input: Math.round(input * f),
            output: Math.round(output * f),
            cacheRead: Math.round(cacheRead * f),
            cacheWrite: Math.round(cacheWrite * f),
            reasoning: Math.round(reasoning * f),
            messages: Math.round(turns * 2 * f),
            cost: c * f,
            startedMs: start,
            lastActivityMs: start + Math.floor(rnd() * 3) * 3_600_000,
            title: TITLES[Math.floor(rnd() * TITLES.length)],
            ledgered: d < 20,
          });
        }
      }
    }
  }
  const byokDaily = [] as UsageDashboard["byokDaily"];
  for (let d = 30; d >= 0; d -= 3) {
    const day = new Date(today);
    day.setDate(day.getDate() - d);
    byokDaily.push({
      date: key(day),
      provider: "openai",
      model: "gpt-5.5",
      input: 12_000 + Math.round(rnd() * 30_000),
      output: 3_000 + Math.round(rnd() * 8_000),
      cost: 0.12 + rnd() * 0.6,
      requests: 3 + Math.floor(rnd() * 9),
    });
  }
  const sum = (k: keyof DailyBucket) => daily.reduce((a, r) => a + (r[k] as number), 0);
  const roll = <K extends string>(pick: (r: DailyBucket) => string, keyName: K) => {
    const m = new Map<string, DailyBucket>();
    for (const r of daily) {
      const cur = m.get(pick(r)) ?? {
        ...r,
        input: 0,
        output: 0,
        cacheRead: 0,
        cacheWrite: 0,
        reasoning: 0,
        cost: 0,
        messages: 0,
        sessions: 0,
      };
      cur.input += r.input;
      cur.output += r.output;
      cur.cacheRead += r.cacheRead;
      cur.cacheWrite += r.cacheWrite;
      cur.reasoning += r.reasoning;
      cur.cost += r.cost;
      cur.messages += r.messages;
      cur.sessions += r.sessions;
      m.set(pick(r), cur);
    }
    return [...m.entries()].map(([k, v]) => ({
      [keyName]: k,
      input: v.input,
      output: v.output,
      cacheRead: v.cacheRead,
      cacheWrite: v.cacheWrite,
      reasoning: v.reasoning,
      cost: v.cost,
      messages: v.messages,
      sessions: v.sessions,
    }));
  };
  const first = new Date(today);
  first.setDate(first.getDate() - (days - 1));
  const ledgerSince = new Date(today);
  ledgerSince.setDate(ledgerSince.getDate() - 19);
  return {
    totals: {
      input: sum("input"),
      output: sum("output"),
      cacheRead: sum("cacheRead"),
      cacheWrite: sum("cacheWrite"),
      reasoning: sum("reasoning"),
      cost: sum("cost"),
      messages: sum("messages"),
      sessions: sessions.length,
      byokInput: byokDaily.reduce((a, b) => a + b.input, 0),
      byokOutput: byokDaily.reduce((a, b) => a + b.output, 0),
      byokCost: byokDaily.reduce((a, b) => a + b.cost, 0),
      byokRequests: byokDaily.reduce((a, b) => a + b.requests, 0),
      totalTokens: sum("input") + sum("output"),
      totalCostUsd: sum("cost"),
    },
    projects: PROJECTS.map(([projectPath, projectName]) => ({
      projectPath,
      projectName,
      firstActivityMs: first.getTime(),
      lastActivityMs: today.getTime(),
      ...(roll((r) => r.projectPath, "projectPath").find(
        (p) => p.projectPath === projectPath,
      ) as any),
    })),
    agents: roll((r) => r.agent, "agent") as any,
    models: roll((r) => r.model, "model") as any,
    daily,
    sessions: sessions.sort((a, b) => (b.lastActivityMs ?? 0) - (a.lastActivityMs ?? 0)),
    sessionsTotal: sessions.length,
    byokDaily,
    byokSince: byokDaily[0]?.date ?? null,
    byokProjectPath: "byok",
    ledgerSince: ledgerSince.toISOString(),
    generatedAt: today.toISOString(),
  };
}

const TITLES = [
  "Rename Console to Usage and add per-turn ledger",
  "Fix the streaming tail freeze in TransientMarkdown",
  "Windows .msi build + atlas-process spawn helper",
  "Landing page: Windows download button",
  "Investigate npm optional dependency wedge",
  "Timeline split into inbox layout",
  "Add team chat markdown rendering",
  "Usage pill in the composer",
  "Org switch comms generation guards",
  "Terminal line emulator rework",
];
