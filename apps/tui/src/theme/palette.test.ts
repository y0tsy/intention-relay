//! The palette's own rules: opacity, distinctness, one theme per frame.

import assert from "node:assert/strict";
import test from "node:test";
import { DARK, LIGHT, type Palette, paletteFor } from "./palette";

/** Every role but the documented alias, in declaration order. */
const ROLES: ReadonlyArray<keyof Omit<Palette, "error" | "scarlet">> = [
  "canvas",
  "panel",
  "codeSurface",
  "headerBg",
  "tabActiveBg",
  "rowSelectedBg",
  "rowAltBg",
  "stubBg",
  "inputBg",
  "badgeBg",
  "badgeInk",
  "userSurface",
  "userBorder",
  "userLabel",
  "selectionBg",
  "toolSurface",
  "toolBorder",
  "toolLabel",
  "toolPreview",
  "noticeSurface",
  "noticeInk",
  "scrollTrack",
  "scrollThumb",
  "dividerTint",
  "border",
  "accent",
  "accentDeep",
  "markdownHeading",
  "markdownCode",
  "markdownLink",
  "reasoningBody",
  "reasoningHeader",
  "timerInk",
  "ink",
  "inkMuted",
  "inkFaint",
  "success",
  "warning",
];

/** Returns one color's relative luminance, as WCAG 2.2 defines it. */
function luminance(hex: string): number {
  const channels = [1, 3, 5].map((offset) => {
    const value = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * (channels[0] ?? 0) + 0.7152 * (channels[1] ?? 0) + 0.0722 * (channels[2] ?? 0);
}

/** Returns the contrast ratio between two colors. */
function contrast(a: string, b: string): number {
  const [lighter, darker] = [luminance(a), luminance(b)].sort((left, right) => right - left);
  return ((lighter ?? 0) + 0.05) / ((darker ?? 0) + 0.05);
}

test("every role is a fully opaque hex triple", () => {
  for (const palette of [LIGHT, DARK]) {
    for (const role of [...ROLES, "error" as const, "scarlet" as const]) {
      assert.match(palette[role], /^#[0-9A-F]{6}$/, role);
    }
  }
});

test("a theme names no two roles alike, error aliasing scarlet", () => {
  for (const [name, palette] of [
    ["light", LIGHT],
    ["dark", DARK],
  ] as const) {
    assert.equal(palette.error, palette.scarlet, `${name}: error is scarlet`);
    const values = ROLES.map((role) => palette[role]);
    assert.equal(new Set(values).size, values.length, `${name}: roles are distinct`);
  }
});

test("a frame is wholly one theme: every role differs between the two", () => {
  for (const role of [...ROLES, "error" as const, "scarlet" as const]) {
    assert.notEqual(LIGHT[role], DARK[role], role);
  }
});

test("the dark page stays neutral while its state inks keep their hue", () => {
  const channels = (value: string): number[] =>
    [1, 3, 5].map((offset) => Number.parseInt(value.slice(offset, offset + 2), 16));
  const [successRed, successGreen, successBlue] = channels(DARK.success);
  assert.ok(
    (successGreen ?? 0) > (successRed ?? 0) && (successGreen ?? 0) > (successBlue ?? 0),
    "success reads green",
  );
  const [warningRed, warningGreen, warningBlue] = channels(DARK.warning);
  assert.ok(
    (warningRed ?? 0) > (warningBlue ?? 0) && (warningGreen ?? 0) > (warningBlue ?? 0),
    "warning reads amber",
  );
  const [errorRed, errorGreen, errorBlue] = channels(DARK.error);
  assert.ok(
    (errorRed ?? 0) > (errorGreen ?? 0) && (errorRed ?? 0) > (errorBlue ?? 0),
    "failure reads red",
  );
  for (const role of ["canvas", "panel", "codeSurface", "border", "ink", "inkMuted"] as const) {
    const [red, green, blue] = channels(DARK[role]);
    assert.equal(red, green, `dark ${role} has no hue`);
    assert.equal((blue ?? 0) - (red ?? 0), 2, `dark ${role} keeps the cool bias`);
  }
});

test("the body and its muted second read carry the frame's text", () => {
  for (const [name, palette] of [
    ["light", LIGHT],
    ["dark", DARK],
  ] as const) {
    assert.ok(contrast(palette.ink, palette.canvas) >= 7, `${name}: body ink holds`);
    assert.ok(contrast(palette.inkMuted, palette.canvas) >= 4.5, `${name}: muted ink holds`);
  }
});

test("a label reads on the wash it sits on", () => {
  // The floor is the 3:1 UI one: the badge and tool washes clear 4.5, the user
  // label on dark is the design's own 4.0 and stays a deviation, not a fix.
  const pairs = [
    ["badgeInk", "badgeBg"],
    ["userLabel", "userSurface"],
    ["toolLabel", "toolSurface"],
    ["toolPreview", "toolSurface"],
    ["noticeInk", "noticeSurface"],
    ["markdownCode", "codeSurface"],
  ] as const;
  for (const [name, palette] of [
    ["light", LIGHT],
    ["dark", DARK],
  ] as const) {
    for (const [label, wash] of pairs) {
      const ratio = contrast(palette[label], palette[wash]);
      assert.ok(ratio >= 3, `${name} ${label} on ${wash} is ${ratio.toFixed(2)}:1`);
    }
  }
});

test("a preference resolves to the palette it names", () => {
  assert.equal(paletteFor("dark"), DARK);
  assert.equal(paletteFor("light"), LIGHT);
});
