// Modified by FubuKing from upstream Atlas (Apache-2.0).
import { describe, expect, it } from "vitest";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { ICONS_DIR, readManifest, staleOutputs } from "../scripts/app-icons.mjs";

/**
 * Guards the selectable macOS app icons (`src-tauri/icons/app-icons/`).
 *
 * FubuKing draws its icons as SVG sources and renders them with the Tauri CLI
 * (`scripts/app-icons.mjs`); the default becomes Tauri's standard icon set. Edit
 * a source, forget `bun run icons:render`, and every build ships the old icon
 * without a word. Likewise an id in the manifest with no rendered `.icns` shows up in
 * Settings and silently falls back to the default when picked.
 *
 * None of it fails to compile, so it is checked here: the manifest, the
 * sources, the committed renders and the Tauri config all have to agree.
 */

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifest = readManifest();
const ids = manifest.icons.map((icon) => icon.id);

function readJson(rel: string) {
  return JSON.parse(readFileSync(path.join(REPO_ROOT, rel), "utf8"));
}

describe("app icon manifest", () => {
  it("lists unique plain ids, and the default is one of them", () => {
    expect(ids.length).toBeGreaterThanOrEqual(1);
    expect(new Set(ids).size).toBe(ids.length);
    // Same rule as `app_icon::is_valid_id`: the id names a file.
    for (const id of ids) expect(id).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(ids).toContain(manifest.default);
  });

  it("has exactly one SVG source per id", () => {
    const sources = readdirSync(path.join(ICONS_DIR, "sources")).filter((n) => n !== ".DS_Store");
    expect(sources.sort()).toEqual(ids.map((id) => `${id}.svg`).sort());
  });

  it("has a rendered .icns for every id but the default, and nothing else", () => {
    const dock = readdirSync(path.join(ICONS_DIR, "dock")).filter((n) => n !== ".DS_Store");
    const expected = ids.filter((id) => id !== manifest.default).map((id) => `${id}.icns`);
    expect(dock.sort()).toEqual(expected.sort());
  });

  it("has renders that match the current sources", () => {
    expect(staleOutputs(manifest)).toEqual([]);
  });
});

describe("tauri config", () => {
  const conf = readJson("src-tauri/tauri.conf.json");
  const macos = readJson("src-tauri/tauri.macos.conf.json");

  it("bundles the rendered Icon.icns, never an .icon source or a compiled car", () => {
    const icons: string[] = conf.bundle.icon;
    expect(icons).toContain("icons/Icon.icns");
    expect(existsSync(path.join(REPO_ROOT, "src-tauri", "icons/Icon.icns"))).toBe(true);
    // An `.icon` entry makes Tauri run actool at bundle time, which needs Xcode.
    expect(icons.filter((i) => i.endsWith(".icon") || i.endsWith(".car"))).toEqual([]);
  });

  it("ships the rendered icons on macOS only, where app_icon.rs reads them", () => {
    expect(macos.bundle.resources).toEqual({ "icons/app-icons/dock/": "icons/app-icons/" });
    const shared = Object.keys(conf.bundle.resources ?? {});
    expect(shared.filter((k) => k.includes("app-icons") || k.endsWith(".icns"))).toEqual([]);
  });
});
