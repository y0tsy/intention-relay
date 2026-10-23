//! The store: one reducer, one client, one live stream.
//!
//! Every test drives the store through the same door a view uses - `dispatch`
//! and a subscription - against a stand-in daemon on a real socket, so what a
//! test asserts is what the running app reaches.

import assert from "node:assert/strict";
import type { Socket } from "node:net";
import test from "node:test";
import type { ErrorDto } from "../bindings";
import { type FakeDaemon, withDaemon } from "../testing/daemon-stand-in";
import { delta, row, run, SESSION_ID, snapshot } from "../testing/fixtures";
import type { ThemePreference } from "./action";
import { AppStore } from "./store";

/** Builds one store over a client, with a recorder a test can read. */
function storeOver(
  client: ConstructorParameters<typeof AppStore>[0]["client"],
  options: { initialSession?: string | null; theme?: ThemePreference } = {},
): { store: AppStore; written: ThemePreference[] } {
  const written: ThemePreference[] = [];
  const store = new AppStore({
    client,
    workspaceRoot: "/workspace",
    mode: null,
    initialSession: options.initialSession ?? null,
    theme: options.theme ?? "auto",
    recorder: {
      writeTheme: async (preference) => {
        written.push(preference);
      },
    },
  });
  return { store, written };
}

/** Returns the kind of the request the daemon just read. */
function kindOf(request: Record<string, unknown>): string {
  return (request.request as { kind: string }).kind;
}

/** The connections one stand-in daemon saw the client subscribe on. */
interface Subscriptions {
  readonly sockets: Socket[];
}

/** Returns the connection the newest subscription arrived on. */
function streamOf(subscriptions: Subscriptions): Socket {
  const socket = subscriptions.sockets.at(-1);
  assert.ok(socket, "the client subscribed");
  return socket;
}

/** Reports whether the client has sent one request of a kind. */
function sent(daemon: FakeDaemon, kind: string): boolean {
  return daemon.requests.some((request) => kindOf(request) === kind);
}

/** Answers a bootstrap with a ready daemon, and the rest with `answer`. */
function handlerFor(
  daemon: FakeDaemon,
  snapshotOf: () => unknown,
  answer: (request: Record<string, unknown>, socket: Socket, id: number) => boolean,
): Subscriptions {
  const subscriptions: Subscriptions = { sockets: [] };
  daemon.handler = (request, socket) => {
    const id = request.id as number;
    switch (kindOf(request)) {
      case "get_daemon_health":
        daemon.reply(socket, id, { kind: "daemon_health", data: {} });
        return;
      case "get_session_snapshot":
        daemon.reply(socket, id, { kind: "session_snapshot", data: snapshotOf() });
        return;
      case "subscribe_session":
        subscriptions.sockets.push(socket);
        daemon.reply(socket, id, { kind: "session_subscribed", data: snapshotOf() });
        return;
      default:
        answer(request, socket, id);
    }
  };
  return subscriptions;
}

/** Waits until the predicate holds, or fails the test. */
async function until(predicate: () => boolean, label: string): Promise<void> {
  for (let attempt = 0; attempt < 400; attempt += 1) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.fail(`timed out waiting for ${label}`);
}

test("a bootstrapped store opens the session it was told to open", async () => {
  await withDaemon(async (daemon, client) => {
    const { store } = storeOver(client, { initialSession: SESSION_ID });
    handlerFor(
      daemon,
      () => snapshot([row(1, "user", "hello")], run("running")),
      () => false,
    );
    store.dispatch({ kind: "connect_requested" });
    await until(() => store.state.stream === "live", "the stream to open");
    assert.equal(store.state.sessionId, SESSION_ID);
    assert.equal(store.state.transcript.length, 1);
    assert.equal(store.state.runStatus, "running");
    store.dispose();
  });
});

test("a frame the daemon sends reaches the transcript", async () => {
  await withDaemon(async (daemon, client) => {
    const { store } = storeOver(client, { initialSession: SESSION_ID });
    const subscriptions = handlerFor(
      daemon,
      () => snapshot([], run("running")),
      () => false,
    );
    store.dispatch({ kind: "connect_requested" });
    await until(() => store.state.stream === "live", "the stream to open");
    daemon.frame(streamOf(subscriptions), {
      kind: "text_delta",
      data: delta(1, "answer", "partial"),
    });
    await until(() => store.state.provisionalText === "partial", "the delta to land");
    daemon.frame(streamOf(subscriptions), {
      kind: "content",
      data: row(7, "assistant", "an answer"),
    });
    await until(() => store.state.transcript.length === 1, "the row to commit");
    assert.equal(store.state.transcript[0]?.text, "an answer");
    assert.equal(store.state.provisionalText, "");
    store.dispose();
  });
});

test("a prompt with no session creates one and sends it in the same command", async () => {
  await withDaemon(async (daemon, client) => {
    const { store } = storeOver(client);
    handlerFor(
      daemon,
      () => snapshot([], run("running")),
      (request, socket, id) => {
        assert.equal(kindOf(request), "create_session");
        assert.deepEqual((request.request as { data: { first_turn: unknown } }).data.first_turn, {
          content: "hello",
        });
        daemon.reply(socket, id, {
          kind: "session_created",
          data: { project_id: "p", workspace_id: "w", session_id: SESSION_ID, first_turn: null },
        });
        return true;
      },
    );
    store.dispatch({ kind: "connect_requested" });
    await until(() => daemon.requests.length > 0, "the daemon to be reached");
    store.dispatch({ kind: "turn_submitted", content: "hello" });
    await until(() => store.state.sessionId !== null, "the created session");
    assert.equal(store.state.error, null);
    store.dispose();
  });
});

test("a prompt with a session is one user turn", async () => {
  await withDaemon(async (daemon, client) => {
    const { store } = storeOver(client, { initialSession: SESSION_ID });
    const subscriptions = handlerFor(
      daemon,
      () => snapshot([], null),
      (request, socket, id) => {
        assert.equal(kindOf(request), "send_user_turn");
        daemon.reply(socket, id, {
          kind: "turn_accepted",
          data: {
            session_id: SESSION_ID,
            turn_id: "t",
            outcome: { kind: "started", run_id: "r" },
          },
        });
        return true;
      },
    );
    store.dispatch({ kind: "connect_requested" });
    await until(() => store.state.stream === "live", "the stream to open");
    store.dispatch({ kind: "turn_submitted", content: "hello" });
    await until(() => sent(daemon, "send_user_turn"), "the turn to leave");
    // The run a turn starts reaches the state as a status frame on the stream
    // that is already open, so no second subscription is asked for.
    assert.equal(subscriptions.sockets.length, 1);
    daemon.frame(streamOf(subscriptions), { kind: "status", data: run("running") });
    await until(() => store.state.runStatus === "running", "the run to appear");
    assert.equal(store.state.error, null);
    store.dispose();
  });
});

test("a failure the client reports becomes the error line", async () => {
  await withDaemon(async (daemon, client) => {
    const { store } = storeOver(client, { initialSession: SESSION_ID });
    daemon.handler = (request, socket) => {
      const id = request.id as number;
      if (kindOf(request) === "get_daemon_health") {
        daemon.reply(socket, id, { kind: "daemon_health", data: {} });
        return;
      }
      daemon.reject(socket, id, "session_not_found");
    };
    store.dispatch({ kind: "connect_requested" });
    await until(() => store.state.error !== null, "the error line");
    assert.match(store.state.error ?? "", /session_not_found/);
    assert.equal(store.state.stream, "inactive");
    store.dispose();
  });
});

test("an effect that fails before the daemon reports its own failure", async () => {
  await withDaemon(async (_daemon, client) => {
    const { store } = storeOver(client);
    store.dispatch({
      kind: "effect_failed",
      error: {
        code: "runtime_directory_unavailable",
        category: "unavailable",
        message: "no runtime directory",
        retryable: false,
      } satisfies ErrorDto,
    });
    assert.equal(store.state.error, "runtime_directory_unavailable: no runtime directory");
    store.dispose();
  });
});

test("the picker writes the theme it commits", async () => {
  await withDaemon(async (_daemon, client) => {
    const { store, written } = storeOver(client, { theme: "auto" });
    store.dispatch({ kind: "theme_requested" });
    assert.equal(store.state.surface, "theme");
    store.dispatch({ kind: "theme_selected", preference: "dark" });
    await until(() => written.length === 1, "the preference to be written");
    assert.deepEqual(written, ["dark"]);
    assert.equal(store.state.theme, "dark");
    assert.equal(store.state.surface, "chat");
    store.dispose();
  });
});

test("a written preference that fails lands on the error line", async () => {
  await withDaemon(async (_daemon, client) => {
    const written: ThemePreference[] = [];
    const store = new AppStore({
      client,
      workspaceRoot: "/workspace",
      mode: null,
      initialSession: null,
      theme: "auto",
      recorder: {
        writeTheme: async (preference) => {
          written.push(preference);
          throw new Error("read-only configuration directory");
        },
      },
    });
    store.dispatch({ kind: "theme_selected", preference: "light" });
    await until(() => store.state.error !== null, "the error line");
    assert.match(store.state.error ?? "", /read-only configuration directory/);
    assert.deepEqual(written, ["light"]);
    store.dispose();
  });
});

test("the browser reads the sessions the daemon lists", async () => {
  await withDaemon(async (daemon, client) => {
    const { store } = storeOver(client);
    daemon.handler = (request, socket) => {
      const id = request.id as number;
      assert.equal(kindOf(request), "list_sessions");
      daemon.reply(socket, id, {
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
              workspace_root: "/workspace",
              message_count: 3,
            },
          ],
        },
      });
    };
    store.dispatch({ kind: "sessions_requested" });
    assert.equal(store.state.surface, "sessions");
    await until(() => store.state.sessions !== null, "the list to arrive");
    assert.equal(store.state.sessions?.sessions.length, 1);
    assert.equal(store.state.selectedSession, SESSION_ID);
    store.dispose();
  });
});

test("a disposed store stops performing effects", async () => {
  await withDaemon(async (daemon, client) => {
    const { store } = storeOver(client);
    store.dispose();
    store.dispatch({ kind: "connect_requested" });
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.deepEqual(daemon.requests, []);
  });
});
