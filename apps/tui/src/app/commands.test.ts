//! The slash grammar: one registry, one parse, one notice.

import assert from "node:assert/strict";
import test from "node:test";
import { COMMANDS, notice, resolveCommand, themeValue, typedName } from "./commands";

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
  assert.equal(invocation("sessions").spec.action, "list_sessions");
  assert.equal(invocation("theme").spec.action, "choose_theme");
  assert.equal(invocation("quit").spec.action, "quit");
});

test("a session is opened by the browser, never by a typed line", () => {
  assert.equal(
    notice(resolveCommand("session 0b1a2c3d")),
    "unknown command /session; known commands: /new /sessions /theme /quit",
  );
});

test("a word past the last declared argument is unexpected", () => {
  assert.equal(notice(resolveCommand("quit now")), 'unexpected argument "now"');
  assert.equal(notice(resolveCommand("new extra")), 'unexpected argument "extra"');
});

test("an unknown name answers with the registry it resolved against", () => {
  assert.equal(
    notice(resolveCommand("watch")),
    "unknown command /watch; known commands: /new /sessions /theme /quit",
  );
});

test("a resolved line carries no notice", () => {
  assert.equal(notice(resolveCommand("new")), null);
});

test("the registry names each command once", () => {
  const names = COMMANDS.map(typedName);
  assert.deepEqual(names, ["/new", "/sessions", "/theme", "/quit"]);
  assert.equal(new Set(names).size, names.length);
});

test("the theme command takes an optional declared value", () => {
  assert.equal(invocation("theme").words.length, 0, "a bare /theme opens the picker");
  assert.equal(invocation("theme DARK").words[0], "DARK");
});

test("the declared theme vocabulary is matched without regard to case", () => {
  assert.equal(themeValue("DARK"), "dark");
  assert.equal(themeValue(" auto "), "auto");
  assert.equal(themeValue("sepia"), undefined);
});

test("the sessions command takes no argument", () => {
  assert.equal(invocation("sessions").spec.action, "list_sessions");
  assert.equal(notice(resolveCommand("sessions now")), 'unexpected argument "now"');
});

test("whitespace around and between words does not reach the transition", () => {
  assert.deepEqual(invocation("  theme   dark  ").words, ["dark"]);
});
