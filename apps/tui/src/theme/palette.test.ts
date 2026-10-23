//! The palette: roles, not colors, and every role legible on its own background.

import assert from "node:assert/strict";
import test from "node:test";
import { DARK, LIGHT, paletteFor, type Palette } from "./palette";

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

/** Every role that paints text, and the floor it has to clear. */
const TEXT_ROLES: ReadonlyArray<keyof Palette> = [
  "foreground",
  "muted",
  "faint",
  "accent",
  "user",
  "assistant",
  "tool",
  "error",
  "warning",
  "success",
  "code",
  "link",
  "quote",
  "provisional",
];

test("every text role clears the 4.5:1 floor on its own background", () => {
  for (const [name, palette] of [
    ["dark", DARK],
    ["light", LIGHT],
  ] as const) {
    for (const role of TEXT_ROLES) {
      const ratio = contrast(palette[role], palette.background);
      assert.ok(ratio >= 4.5, `${name} ${role} is ${ratio.toFixed(2)}:1`);
    }
  }
});

test("the frame's border is visible against the background it draws on", () => {
  // A border is a non-text boundary, so the floor is the 3:1 UI-component one.
  for (const [name, palette] of [
    ["dark", DARK],
    ["light", LIGHT],
  ] as const) {
    const ratio = contrast(palette.border, palette.background);
    assert.ok(ratio >= 3, `${name} border is ${ratio.toFixed(2)}:1`);
  }
});

test("text that sits on the accent is legible on it", () => {
  for (const [name, palette] of [
    ["dark", DARK],
    ["light", LIGHT],
  ] as const) {
    const ratio = contrast(palette.onAccent, palette.accent);
    assert.ok(ratio >= 4.5, `${name} on-accent is ${ratio.toFixed(2)}:1`);
  }
});

test("the roles stay distinguishable from one another", () => {
  for (const [name, palette] of [
    ["dark", DARK],
    ["light", LIGHT],
  ] as const) {
    assert.ok(
      contrast(palette.faint, palette.background) < contrast(palette.muted, palette.background),
      `${name}: faint reads dimmer than muted`,
    );
    assert.ok(
      contrast(palette.muted, palette.background) <
        contrast(palette.foreground, palette.background),
      `${name}: muted reads dimmer than the body`,
    );
    assert.ok(
      contrast(palette.border, palette.background) < contrast(palette.accent, palette.background),
      `${name}: the frame is quieter than the accent`,
    );
  }
});

test("a preference resolves to the palette it names", () => {
  assert.equal(paletteFor("dark"), DARK);
  assert.equal(paletteFor("light"), LIGHT);
});
