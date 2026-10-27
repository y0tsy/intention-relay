//! The theme file: what it declares, and what a write leaves behind.

import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  configPath,
  parseThemePreference,
  readThemePreference,
  replaceThemeLine,
  writeThemePreference,
} from "./config";
import { DARK, LIGHT, paletteFor } from "./palette";

test("a declared preference is read from its own section", () => {
  assert.equal(parseThemePreference('[tui]\ntheme = "dark"\n'), "dark");
  assert.equal(parseThemePreference("[tui]\ntheme = 'light'\n"), "light");
  assert.equal(parseThemePreference('[tui]\ntheme = "auto"\n'), "auto");
});

test("a key outside the section or of an unknown value declares nothing", () => {
  assert.equal(parseThemePreference('theme = "dark"\n'), null);
  assert.equal(parseThemePreference('[other]\ntheme = "dark"\n'), null);
  assert.equal(parseThemePreference('[tui]\ntheme = "sepia"\n'), null);
  assert.equal(parseThemePreference(""), null);
  assert.equal(parseThemePreference('# [tui]\n# theme = "dark"\n'), null);
});

test("a file that declares nothing keeps its other lines when the theme is written", () => {
  const original = '[provider]\nmodel = "m"\n\n[tui]\nhr = true\n';
  assert.equal(
    replaceThemeLine(original, "light"),
    '[provider]\nmodel = "m"\n\n[tui]\nhr = true\ntheme = "light"\n',
  );
});

test("a file with no section gains one", () => {
  assert.equal(replaceThemeLine("", "dark"), '[tui]\ntheme = "dark"\n');
  assert.equal(
    replaceThemeLine('[provider]\nmodel = "m"\n', "dark"),
    '[provider]\nmodel = "m"\n[tui]\ntheme = "dark"\n',
  );
});

test("an existing theme line is replaced in place", () => {
  assert.equal(
    replaceThemeLine('# note\n[tui]\ntheme = "auto"\nother = 1\n', "dark"),
    '# note\n[tui]\ntheme = "dark"\nother = 1\n',
  );
});

test("a write replaces the preference the next read reports", async () => {
  const directory = await mkdtemp(join(tmpdir(), "intention-tui-theme-"));
  const path = join(directory, "nested", "tui.toml");
  try {
    assert.equal(await readThemePreference(path), "auto", "an absent file reads as auto");
    await writeThemePreference("dark", path);
    assert.equal(await readThemePreference(path), "dark");
    await writeFile(path, '[tui]\ntheme = "dark"\nkeep = 1\n', "utf8");
    await writeThemePreference("light", path);
    assert.equal(await readFile(path, "utf8"), '[tui]\ntheme = "light"\nkeep = 1\n');
    await assert.doesNotReject(() => writeThemePreference("auto", path));
    assert.equal(await readThemePreference(path), "auto");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("the configuration path honours a declared directory", () => {
  assert.equal(
    configPath({ INTENTION_CONFIG_DIR: "/tmp/relay", HOME: "/home/user" }),
    join("/tmp/relay", "tui.toml"),
  );
  assert.equal(
    configPath({ HOME: "/home/user" }),
    join("/home/user", ".config", "intention-relay", "tui.toml"),
  );
});

test("both palettes name the same roles", () => {
  assert.deepEqual(Object.keys(DARK).sort(), Object.keys(LIGHT).sort());
  assert.equal(paletteFor("dark"), DARK);
  assert.equal(paletteFor("light"), LIGHT);
});
