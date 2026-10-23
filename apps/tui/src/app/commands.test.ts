//! The slash grammar: one registry, one parse, one notice.

import assert from "node:assert/strict";
import test from "node:test";
import { COMMANDS, notice, resolveCommand, typedName } from "./commands";

/** Resolves one line and returns its invocation, failing on a notice. */
function invocation(line: string) {
  const resolution = resolveCommand(line);
  assert.equal(resolution.kind, "invocation", `/${line} resolves`);
  if (resolution.kind !== "invocation") {
    throw new Error("unreachable");
  }
  return resolution.invocation;
}

test("every command resolves from its typed name", () => {
  assert.equal(invocation("new").spec.action, "new_session");
  assert.equal(invocation("quit").spec.action, "quit");
  assert.equal(invocation("session 0b1a2c3d").spec.action, "open_session");
});

test("the identifier reaches the transition as it was typed", () => {
  assert.deepEqual(invocation("session 0b1a2c3d-0000-4000-8000-000000000001").words, [
    "0b1a2c3d-0000-4000-8000-000000000001",
  ]);
});

test("a required argument the line omits is named in the notice", () => {
  const resolution = resolveCommand("session");
  assert.equal(notice(resolution), 'missing argument "session"');
});

test("a word past the last declared argument is unexpected", () => {
  assert.equal(notice(resolveCommand("quit now")), 'unexpected argument "now"');
  assert.equal(notice(resolveCommand("new extra")), 'unexpected argument "extra"');
});

test("an unknown name answers with the registry it resolved against", () => {
  assert.equal(
    notice(resolveCommand("watch")),
    "unknown command /watch; known commands: /new /session /quit",
  );
});

test("a resolved line carries no notice", () => {
  assert.equal(notice(resolveCommand("new")), null);
});

test("the registry names each command once", () => {
  const names = COMMANDS.map(typedName);
  assert.deepEqual(names, ["/new", "/session", "/quit"]);
  assert.equal(new Set(names).size, names.length);
});

test("whitespace around and between words does not reach the transition", () => {
  assert.deepEqual(invocation("  session   0b1a2c3d  ").words, ["0b1a2c3d"]);
});
