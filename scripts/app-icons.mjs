#!/usr/bin/env node
// Modified by FubuMem from upstream Atlas (Apache-2.0).
/**
 * Renders FubuMem's app icons from their SVG sources.
 *
 *   node scripts/app-icons.mjs            re-render everything
 *   node scripts/app-icons.mjs --check    fail if a committed output is stale
 *
 * Source of truth: `src-tauri/icons/app-icons/app-icons.json` (ids, labels,
 * default) plus one `sources/<id>.svg` per id, drawn on Apple's macOS grid (an
 * 824pt body centred on a 1024pt canvas). Outputs, all committed:
 *
 *   src-tauri/icons/*           the DEFAULT icon as Tauri's standard set
 *                               (`Icon.icns`, `icon.ico`, PNGs, iOS, Android),
 *                               rendered by `tauri icon`, plus the DMG icon and
 *                               the 1024px and 256px PNGs derived from it.
 *   app-icons/dock/<id>.icns    every OTHER icon, applied at runtime by
 *                               `src-tauri/src/app_icon.rs`.
 *   app-icons/sources.sha256    hash of the default id + every source file, so
 *                               `--check` can tell an edited source from a
 *                               re-rendered one.
 *
 * Upstream Atlas compiled Icon Composer sources with Xcode 26's actool into an
 * `Assets.car`. FubuMem draws its icons as SVG and renders them with the
 * Tauri CLI (resvg), so no Xcode install is needed to change an icon.
 */
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TAURI_ICONS_DIR = path.join(REPO_ROOT, "src-tauri", "icons");
export const ICONS_DIR = path.join(TAURI_ICONS_DIR, "app-icons");
const COMMAND = "bun run icons:render";

/** @returns {{ default: string, icons: { id: string, label: string }[] }} */
export function readManifest() {
  return JSON.parse(readFileSync(path.join(ICONS_DIR, "app-icons.json"), "utf8"));
}

/** Every file under `dir`, relative to it, sorted; Finder litter skipped. */
function listFiles(dir, prefix = "") {
  const out = [];
  for (const name of readdirSync(dir).sort()) {
    if (name === ".DS_Store") continue;
    const full = path.join(dir, name);
    const rel = prefix ? `${prefix}/${name}` : name;
    if (statSync(full).isDirectory()) out.push(...listFiles(full, rel));
    else out.push(rel);
  }
  return out;
}

/**
 * sha256 over the default id and every source file's path and bytes. The
 * labels are left out on purpose: renaming one in Settings needs no re-render.
 */
export function sourcesHash(manifest = readManifest()) {
  const hash = createHash("sha256");
  hash.update(`default:${manifest.default}\n`);
  const sources = path.join(ICONS_DIR, "sources");
  for (const rel of listFiles(sources)) {
    hash.update(`${rel}\0`);
    hash.update(readFileSync(path.join(sources, rel)));
    hash.update("\0");
  }
  return hash.digest("hex");
}

/** What `--check` would complain about; empty when every output is current. */
export function staleOutputs(manifest = readManifest()) {
  const problems = [];
  if (!existsSync(path.join(TAURI_ICONS_DIR, "Icon.icns"))) problems.push("Icon.icns is missing");
  for (const { id } of manifest.icons) {
    if (id !== manifest.default && !existsSync(path.join(ICONS_DIR, "dock", `${id}.icns`))) {
      problems.push(`dock/${id}.icns is missing`);
    }
  }
  const stampPath = path.join(ICONS_DIR, "sources.sha256");
  const stamp = existsSync(stampPath) ? readFileSync(stampPath, "utf8").trim() : "";
  if (stamp !== sourcesHash(manifest)) {
    problems.push("sources changed since the last render (sources.sha256 does not match)");
  }
  return problems;
}

function run(cmd, args) {
  // stdin from /dev/null, never inherited: a CLI that reads it would block.
  const r = spawnSync(cmd, args, { stdio: ["ignore", "pipe", "pipe"], encoding: "utf8" });
  if (r.status !== 0) {
    throw new Error(
      `${path.basename(cmd)} failed (${r.status ?? r.signal}):\n${r.stdout}${r.stderr}`,
    );
  }
  return r.stdout;
}

/** Tauri's full icon set for one SVG, written to `outDir`. */
function tauriIcon(id, outDir) {
  run("bunx", ["tauri", "icon", path.join(ICONS_DIR, "sources", `${id}.svg`), "-o", outDir]);
}

/** The default icon into `src-tauri/icons`, plus the files derived from it. */
function renderDefault(id) {
  tauriIcon(id, TAURI_ICONS_DIR);
  const png = path.join(TAURI_ICONS_DIR, "icon.png");
  copyFileSync(png, path.join(TAURI_ICONS_DIR, "app-icon-source.png"));
  run("sips", ["-z", "256", "256", png, "--out", path.join(TAURI_ICONS_DIR, "256x256.png")]);
  copyFileSync(
    path.join(TAURI_ICONS_DIR, "Icon.icns"),
    path.join(TAURI_ICONS_DIR, "dmg-icon.icns"),
  );
}

function render() {
  const manifest = readManifest();
  const ids = manifest.icons.map((i) => i.id);
  if (!ids.includes(manifest.default))
    throw new Error(`default "${manifest.default}" is not in icons`);
  const tmp = mkdtempSync(path.join(tmpdir(), "fubumem-app-icons-"));
  try {
    renderDefault(manifest.default);
    console.log(`app-icons: src-tauri/icons ← sources/${manifest.default}.svg`);
    const dock = path.join(ICONS_DIR, "dock");
    mkdirSync(dock, { recursive: true });
    for (const name of readdirSync(dock)) {
      if (
        !ids.includes(path.basename(name, ".icns")) ||
        path.basename(name, ".icns") === manifest.default
      ) {
        rmSync(path.join(dock, name));
        console.log(`app-icons: removed dock/${name}`);
      }
    }
    for (const id of ids) {
      if (id === manifest.default) continue;
      const out = path.join(tmp, id);
      tauriIcon(id, out);
      copyFileSync(path.join(out, "icon.icns"), path.join(dock, `${id}.icns`));
      console.log(`app-icons: dock/${id}.icns ← sources/${id}.svg`);
    }
    writeFileSync(path.join(ICONS_DIR, "sources.sha256"), `${sourcesHash(manifest)}\n`);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

function check() {
  const problems = staleOutputs();
  if (problems.length === 0) return;
  for (const p of problems) console.error(`app-icons: ${p}`);
  console.error(`app-icons: run \`${COMMAND}\` and commit the result`);
  process.exitCode = 1;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    if (process.argv.includes("--check")) check();
    else render();
  } catch (error) {
    console.error(`app-icons: ${error instanceof Error ? error.message : error}`);
    process.exitCode = 1;
  }
}
