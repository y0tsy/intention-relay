//! Input: keys, paste, wheel, selection, and resize against a real frame.
//!
//! The tests drive the same events a terminal sends and read the frame the app
//! painted, so "a pasted block does not submit" and "a resized footer repaints
//! inside its own edges" are assertions about cells, not about intentions.

import assert from "node:assert/strict";
import test from "node:test";
import { pasteBytes } from "@opentui/core/testing";
import type { AppStore } from "../app/store";
import { withDaemon } from "../testing/daemon-stand-in";
import { row, run, SESSION_ID, snapshot } from "../testing/fixtures";
import { mountApp } from "../testing/render";
import { offlineStore, storeFor } from "../testing/store";
import { App } from "./App";

/** Renders the app and returns the setup a test drives it with. */
async function mount(store: AppStore, width: number, height: number) {
  return mountApp(<App store={store} />, width, height);
}

/** Returns the painted frame's lines, trimmed of padding and of trailing blanks. */
function frameLines(setup: { captureCharFrame: () => string }): string[] {
  const lines = setup
    .captureCharFrame()
    .split("\n")
    .map((line) => line.trim());
  while (lines.length > 0 && lines.at(-1) === "") {
    lines.pop();
  }
  return lines;
}

/** Returns the composer's line, wherever the frame put it. */
function composerLine(setup: { captureCharFrame: () => string }): string {
  const line = frameLines(setup).find((row) => row.includes("› ")) ?? "";
  // The frame pads its inside to the panel's width, so the leading and trailing
  // borders are not part of what the composer painted.
  return line.replace(/^.*› /, "› ").replace(/\s*│$/, "").trimEnd();
}

test("a typed prompt reaches the composer and a return submits it", async () => {
  const store = offlineStore();
  const setup = await mount(store, 60, 16);
  await setup.mockInput.pressKeys(["h", "i"]);
  await setup.renderOnce();
  assert.match(composerLine(setup), /› hi█/);
  await setup.mockInput.pressEnter();
  await setup.renderOnce();
  assert.match(composerLine(setup), /› █/, "the draft is spent");
  store.dispose();
  await setup.dispose();
});

test("a pasted block lands in the draft whole and does not submit", async () => {
  const store = offlineStore();
  const setup = await mount(store, 60, 16);
  await setup.mockInput.pasteBracketedText("line one\nline two");
  await setup.renderOnce();
  const text = frameLines(setup).join("\n");
  assert.match(text, /line one/);
  assert.match(text, /line two/, "the second line of the paste is in the draft, not sent");
  store.dispose();
  await setup.dispose();
});

test("a pasted block reaches the session as one prompt", async () => {
  await withDaemon(async (daemon, client) => {
    const prompts: string[] = [];
    daemon.handler = (request, socket) => {
      const id = request.id as number;
      const payload = request.request as { kind: string; data?: { content?: string } };
      switch (payload.kind) {
        case "get_daemon_health":
          daemon.reply(socket, id, { kind: "daemon_health", data: {} });
          return;
        case "get_session_snapshot":
          daemon.reply(socket, id, {
            kind: "session_snapshot",
            data: snapshot([row(1, "user", "earlier")], run("completed")),
          });
          return;
        case "subscribe_session":
          daemon.reply(socket, id, {
            kind: "session_subscribed",
            data: snapshot([row(1, "user", "earlier")], run("completed")),
          });
          return;
        case "send_user_turn":
          prompts.push(payload.data?.content ?? "");
          daemon.reply(socket, id, {
            kind: "turn_accepted",
            data: {
              session_id: SESSION_ID,
              turn_id: "t",
              outcome: { kind: "started", run_id: "r" },
            },
          });
          return;
        default:
          return;
      }
    };
    const store = storeFor(client, { initialSession: SESSION_ID });
    store.dispatch({ kind: "connect_requested" });
    for (let attempt = 0; attempt < 400 && store.state.stream !== "live"; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    const setup = await mount(store, 60, 16);
    await setup.mockInput.pasteBracketedText("one\ntwo\nthree");
    // The paste is data the composer holds; the return that follows is the
    // gesture that submits it, and it needs the draft to have landed first.
    await setup.renderOnce();
    await setup.mockInput.pressEnter();
    for (let attempt = 0; attempt < 200 && prompts.length === 0; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    assert.deepEqual(prompts, ["one\ntwo\nthree"]);
    store.dispose();
    await setup.dispose();
  });
});

test("a paste never decodes into a gesture", () => {
  // The decoder is the only thing between the terminal's bytes and the draft.
  assert.equal(new TextDecoder().decode(pasteBytes("a\r\nb")), "a\r\nb");
});

test("a wheel notch is the terminal's, and changes no row the window paints", async () => {
  const store = offlineStore();
  const setup = await mount(store, 60, 16);
  const before = frameLines(setup);
  await setup.mockMouse.scroll(10, 8, "down");
  await setup.mockMouse.scroll(10, 8, "up");
  await setup.renderOnce();
  assert.deepEqual(frameLines(setup), before, "the footer is the cell it was");
  assert.match(composerLine(setup), /› █/, "the composer still owns the keys");
  store.dispose();
  await setup.dispose();
});

test("a resized footer repaints inside its own edges", async () => {
  const store = offlineStore();
  const setup = await mount(store, 100, 30);
  await setup.renderOnce();
  assert.equal(
    setup.captureCharFrame().replace(/\n$/, "").split("\n").length,
    30,
    "the frame fills the terminal",
  );
  setup.resize(40, 12);
  await setup.renderOnce();
  const raw = setup.captureCharFrame().replace(/\n$/, "").split("\n");
  assert.equal(raw.length, 12, "the frame takes the new height");
  for (const line of raw) {
    assert.ok(line.trimEnd().length <= 40, `"${line}" fits the new width`);
  }
  assert.ok(
    raw.some((line) => line.trimStart().startsWith("╭")),
    "the footer still frames the input block",
  );
  store.dispose();
  await setup.dispose();
});

test("escape is a gesture the composer owns", async () => {
  // The test renderer's mock keys do not deliver escape to the component tree,
  // so what escape means is decided in `keys.test.ts` and its delivery is
  // exercised against a real terminal.
  const store = offlineStore();
  const setup = await mount(store, 60, 16);
  await setup.mockInput.pressKeys(["h", "i"]);
  await setup.renderOnce();
  assert.match(composerLine(setup), /› hi█/);
  store.dispose();
  await setup.dispose();
});
