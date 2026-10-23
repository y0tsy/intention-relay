//! The application surface: one frame, one keyboard, one live panel.
//!
//! The tests drive a real store over a stand-in daemon and read the frame the
//! app paints, so the surface that owns the keyboard is the surface on screen.

import assert from "node:assert/strict";
import test from "node:test";
import type { AppStore } from "../app/store";
import { withDaemon } from "../testing/daemon-stand-in";
import { row, run, SESSION_ID, snapshot } from "../testing/fixtures";
import { frameLines, painted } from "../testing/render";
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

/** Renders the app at one terminal size and returns its lines. */
async function lines(store: AppStore, width: number, height: number): Promise<string[]> {
  const frame = await painted(<App store={store} />, width, height);
  return frameLines(frame);
}

test("the panel is the one frame the window holds, inside a one-cell margin", async () => {
  const store = offlineStore();
  const frame = await painted(<App store={store} />, 60, 16);
  const raw = frame.replace(/\n$/, "").split("\n");
  assert.equal(raw.length, 16, "the frame fills the terminal");
  assert.equal(raw[0]?.trim(), "", "the window's own margin stays visible above the panel");
  assert.equal(raw.at(-1)?.trim(), "", "and below it");
  assert.match(raw[1]?.trim() ?? "", /^╭─+╮$/, "the panel opens one row down");
  assert.match(raw[14]?.trim() ?? "", /^╰─+╯$/, "and closes one row above the window's last");
  for (const line of raw.slice(1, 15)) {
    const edge = line.trim();
    assert.ok(edge.startsWith("│") || edge.startsWith("╭") || edge.startsWith("╰"), edge);
    assert.ok(edge.endsWith("│") || edge.endsWith("╮") || edge.endsWith("╯"), edge);
  }
  store.dispose();
});

test("an app with nothing open paints the welcome, the input block, and the status row", async () => {
  const store = offlineStore();
  const painted_ = await lines(store, 60, 16);
  const text = painted_.join("\n");
  assert.match(text, /INTENTION {2}R E L A Y/);
  assert.match(text, /Version 0\.0\.0/);
  assert.match(text, /> █/);
  assert.match(text, /Connecting… · no session/);
  assert.match(text, /enter sends \| \/new session \| \/sessions switch/);
  store.dispose();
});

test("the theme picker takes the panel when it is asked for", async () => {
  const store = offlineStore("light");
  store.dispatch({ kind: "theme_requested" });
  const painted_ = await lines(store, 60, 16);
  const text = painted_.join("\n");
  assert.match(text, /Theme/, "the picker's panel is titled");
  assert.match(text, /> light/, "the highlighted preference is the one the store holds");
  assert.match(text, /> light\s+for a light terminal/);
  assert.match(text, /for a light terminal\s+current/);
  assert.match(text, /enter apply \| esc revert/, "the panel carries its own legend");
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
    const painted_ = await lines(store, 60, 16);
    const text = painted_.join("\n");
    assert.match(text, /sessions · 1/);
    assert.match(text, /project\s+3/);
    assert.match(text, /↑↓ move · enter open · esc close/);
    store.dispose();
  });
});

test("the transcript paints the rows the daemon sent", async () => {
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
    const painted_ = await lines(store, 60, 16);
    const text = painted_.join("\n");
    assert.match(text, /❯ you/);
    assert.match(text, /explain this repository/);
    assert.match(text, /It relays intentions\./);
    assert.equal(
      text.includes("intention relay\n"),
      false,
      "the welcome is gone once a row exists",
    );
    store.dispose();
  });
});
