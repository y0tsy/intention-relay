//! The submission resolver: one line in, one transition out.

import assert from "node:assert/strict";
import test from "node:test";
import { resolveSubmission } from "./submit";

test("plain text is a prompt turn", () => {
  assert.deepEqual(resolveSubmission("  hello there  "), {
    kind: "prompt",
    text: "  hello there  ",
  });
});

test("a known command names its transition", () => {
  assert.deepEqual(resolveSubmission("/quit"), { kind: "quit" });
  assert.deepEqual(resolveSubmission("/new"), { kind: "new_session" });
  assert.deepEqual(resolveSubmission("/sessions"), { kind: "list_sessions" });
  assert.deepEqual(resolveSubmission("/theme"), { kind: "theme_picker" });
  assert.deepEqual(resolveSubmission("/theme dark"), {
    kind: "theme",
    preference: "dark",
  });
});

test("an unknown command or theme is a notice, never a turn", () => {
  const command = resolveSubmission("/watch");
  assert.equal(command.kind, "notice");
  assert.match(command.kind === "notice" ? command.text : "", /unknown command/);
  const theme = resolveSubmission("/theme neon");
  assert.equal(theme.kind, "notice");
  assert.match(theme.kind === "notice" ? theme.text : "", /unknown theme/);
});
