//! The application surface: one frame, one keyboard, one live footer.
//!
//! The tests drive a real store over a stand-in daemon and read what the window
//! painted and what it printed into the terminal, so the surface that owns the
//! keyboard is the surface on screen and the transcript is the terminal's.

import assert from "node:assert/strict";
import test from "node:test";
import type { AppStore } from "../app/store";
import { withDaemon } from "../testing/daemon-stand-in";
import { row, run, SESSION_ID, snapshot } from "../testing/fixtures";
import { frameLines, mountApp, printedRows } from "../testing/render";
import { offlineStore, storeFor } from "../testing/store";
import { App } from "./App";

/** Waits until the predicate holds, or gives up. */
async function until(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 400; attempt += 1) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

/** Renders the app at one terminal size and returns the footer's lines. */
async function lines(store: AppStore, width: number, height: number): Promise<string[]> {
  const setup = await mountApp(<App store={store} />, width, height);
  const frame = setup.captureCharFrame();
  await setup.dispose();
  return visible(frameLines(frame));
}

/** Renders the app and returns the footer's lines and the rows it printed. */
async function screen(
  store: AppStore,
  width: number,
  height: number,
): Promise<{ footer: string[]; printed: string[] }> {
  const setup = await mountApp(<App store={store} />, width, height);
  const frame = setup.captureCharFrame();
  const printed = printedRows(setup);
  await setup.dispose();
  return { footer: frameLines(frame), printed };
}

/** Turns the painted page's invisible spaces into ordinary ones. */
function visible(lines: readonly string[]): string[] {
  return lines.map((line) => line.replace(/ /g, " "));
}

test("the window ends on the footer's own last row, with the input block framed in it", async () => {
  const store = offlineStore();
  const setup = await mountApp(<App store={store} />, 60, 16);
  const frame = setup.captureCharFrame();
  await setup.dispose();
  store.dispose();
  const raw = visible(frame.replace(/\n$/, "").split("\n"));
  assert.equal(raw.length, 16, "the footer fills the terminal");
  const painted_ = raw.filter((line) => line.trimEnd().length > 0);
  assert.match(
    painted_.at(-1) ?? "",
    /^\s*╰─+╯\s*$/,
    "the window's frame closes last, so nothing is left open below it",
  );
  const framed = raw.filter((line) => line.includes("╭"));
  // The window's own frame, the overview card, and the input block: the footer
  // carries everything the alt-screen panel did.
  assert.equal(framed.length, 3, "the window, the overview, and the input block frame the rows");
  for (const line of raw) {
    assert.ok(line.trimEnd().length <= 60, `"${line}" fits the window`);
  }
});

test("every row the footer owns carries the page, blank rows included", async () => {
  const store = offlineStore();
  const setup = await mountApp(<App store={store} />, 40, 12);
  const raw = setup.captureCharFrame().replace(/\n$/, "").split("\n");
  await setup.dispose();
  store.dispose();
  assert.equal(raw.length, 12, "the footer fills the terminal");
  for (const [row, line] of raw.entries()) {
    // A terminal paints only the cells an application draws, so a row left empty
    // is a hole through the page: each row holds a glyph in every cell.
    assert.equal(line.length, 40, `row ${row + 1} paints its full width`);
    assert.match(line.replace(/\u00a0/g, "·"), /·/, `row ${row + 1} holds the page`);
  }
});

test("an app with nothing open paints the welcome, the input block, and the status row", async () => {
  const store = offlineStore();
  const painted_ = await lines(store, 60, 18);
  const text = painted_.join("\n");
  assert.match(text, /Ｉ Ｎ Ｔ Ｅ Ｎ Ｔ Ｉ Ｏ Ｎ/);
  assert.match(text, /✦ Ｒ Ｅ Ｌ Ａ Ｙ ✦/);
  assert.match(text, /Version 0\.0\.0/);
  assert.match(text, /> █/);
  assert.match(text, /Connecting… · no session/);
  assert.match(text, /enter sends · \/new · \/sessions/);
  assert.match(text, /A local agent for the long run\./);
  assert.match(text, /\/new start · \/sessions browse · \/theme palette/);
  store.dispose();
});

test("the theme picker takes the panel when it is asked for", async () => {
  const store = offlineStore("light");
  store.dispatch({ kind: "theme_requested" });
  const painted_ = await lines(store, 60, 16);
  const text = painted_.join("\n");
  assert.match(text, /Theme/, "the picker's panel is titled");
  assert.match(text, /> light/, "the highlighted preference is the one the store holds");
  assert.match(text, /> light\s+use the light palette/);
  assert.match(text, /use the light palette\s+current/);
  assert.match(text, /enter apply · esc cancel/, "the panel carries its own legend");
  store.dispose();
});

test("the browser takes the panel and lists what the daemon sent", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (request, socket) => {
      const payload = request.request as { kind: string };
      if (payload.kind === "list_sessions") {
        daemon.reply(socket, request.id as number, {
          kind: "sessions_listed",
          data: {
            sessions: [
              {
                session_id: SESSION_ID,
                project_id: "p",
                workspace_id: "w",
                mode: "build",
                created_at: 1_700_000_000,
                updated_at: 1_700_000_060,
                workspace_root: "/home/user/project",
                message_count: 3,
              },
            ],
            omitted: 0,
          },
        });
      }
    };
    const store = storeFor(client);
    store.dispatch({ kind: "sessions_requested" });
    await until(() => store.state.sessions !== null);
    const painted_ = await lines(store, 90, 34);
    const text = painted_.join("\n");
    assert.match(text, /Sessions/, "the browser is docked under the footer");
    assert.match(text, /⦿ All sessions/);
    assert.match(text, /Enter open · Tab switch list · Esc close/);
    assert.match(text, /\/home\/user/, "the workspace the session is bound to");
    store.dispose();
  });
});

test("the transcript prints the rows the daemon sent, once, above the footer", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (request, socket) => {
      const payload = request.request as { kind: string };
      const id = request.id as number;
      if (payload.kind === "get_daemon_health") {
        daemon.reply(socket, id, { kind: "daemon_health", data: {} });
        return;
      }
      const data = snapshot(
        [row(1, "user", "explain this repository"), row(2, "assistant", "It relays intentions.")],
        run("completed"),
      );
      daemon.reply(socket, id, {
        kind: payload.kind === "subscribe_session" ? "session_subscribed" : "session_snapshot",
        data,
      });
    };
    const store = storeFor(client, { initialSession: SESSION_ID });
    store.dispatch({ kind: "connect_requested" });
    await until(() => store.state.stream === "live");
    const shown = await screen(store, 60, 16);
    // A printed row carries the page between its glyphs, so its blanks are the
    // page's own and read as ordinary spaces here.
    const text = visible(shown.printed).join("\n");
    assert.match(text, /❯ you/);
    assert.match(text, /explain this repository/);
    assert.match(text, /It relays intentions\./);
    assert.equal(
      shown.footer.join("\n").includes("INTENTION"),
      false,
      "the welcome is gone once a row is printed",
    );
    assert.equal(
      shown.printed.filter((line) => line.includes("explain this repository")).length,
      1,
      "a committed row is printed exactly once",
    );
    store.dispose();
  });
});
