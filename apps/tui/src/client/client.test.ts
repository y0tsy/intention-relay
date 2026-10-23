//! The client against a real socket: requests, stream frames, and bootstrap.
//!
//! Every test here runs a small daemon stand-in on a real local socket, so the
//! framing, the correlation, and the stream loop are exercised as they run.

import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { FakeDaemon, withDaemon } from "../testing/daemon-stand-in";
import { row, run, SESSION_ID, snapshot } from "../testing/fixtures";
import { IntentionClient } from "./client";
import type { LocalEndpoint } from "./endpoint";
import { ClientFailure } from "./errors";
import { RETAINED_TRANSCRIPT_MESSAGES } from "./stream";

test("a health request reads the daemon's reply", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (request, socket) => {
      assert.deepEqual(request.request, { kind: "get_daemon_health" });
      daemon.reply(socket, request.id as number, { kind: "daemon_health", data: {} });
    };
    await client.connectOrBootstrap();
    assert.equal(daemon.requests.length, 1);
  });
});

test("a created session returns the identity the daemon accepted", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (request, socket) => {
      daemon.reply(socket, request.id as number, {
        kind: "session_created",
        data: {
          project_id: "p",
          workspace_id: "w",
          session_id: SESSION_ID,
          first_turn: null,
        },
      });
    };
    assert.equal(
      await client.createSession({
        project_id: "p",
        session_id: SESSION_ID,
        workspace_id: "w",
        workspace_root: "/workspace",
        mode: null,
        first_turn: null,
      }),
      SESSION_ID,
    );
  });
});

test("the daemon's rejection reaches the caller as its typed failure", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (request, socket) => {
      daemon.reject(socket, request.id as number, "session_not_found");
    };
    await assert.rejects(
      () => client.sessionSnapshot(SESSION_ID),
      (error: unknown) =>
        error instanceof ClientFailure &&
        error.dto.code === "session_not_found" &&
        error.dto.category === "validation",
    );
  });
});

test("a reply of the wrong kind is a mismatched response", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (request, socket) => {
      daemon.reply(socket, request.id as number, { kind: "daemon_health", data: {} });
    };
    await assert.rejects(
      () => client.sendUserTurn(SESSION_ID, "hello"),
      (error: unknown) =>
        error instanceof ClientFailure && error.dto.code === "invalid_local_protocol_response",
    );
  });
});

test("a correlation the client did not send is a mismatched response", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (_request, socket) => {
      daemon.reply(socket, 99, { kind: "daemon_health", data: {} });
    };
    await assert.rejects(
      () => client.connectOrBootstrap(),
      (error: unknown) =>
        error instanceof ClientFailure && error.dto.code === "invalid_local_protocol_response",
    );
  });
});

test("a stream opens with the snapshot's state and applies the frames that follow", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (request, socket) => {
      if (request.request === undefined) {
        return;
      }
      const payload = request.request as { kind: string };
      if (payload.kind !== "subscribe_session") {
        return;
      }
      daemon.reply(socket, request.id as number, {
        kind: "session_subscribed",
        data: snapshot([row(1, "user", "hello")], run("running")),
      });
      daemon.frame(socket, { kind: "content", data: row(2, "assistant", "hi") });
      daemon.frame(socket, { kind: "status", data: run("completed") });
    };
    const stream = await client.subscribe(SESSION_ID);
    assert.equal(stream.state.messages.length, 1);
    const first = await stream.next();
    assert.equal(first.kind, "frame");
    const second = await stream.next();
    assert.equal(second.kind, "frame");
    assert.deepEqual(
      stream.state.messages.map((message) => message.text),
      ["hello", "hi"],
    );
    assert.equal(stream.state.status, "completed");
    stream.close();
  });
});

test("a stream the daemon closes reports its own classification", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (request, socket) => {
      daemon.reply(socket, request.id as number, {
        kind: "session_subscribed",
        data: snapshot([], null),
      });
      socket.end();
    };
    const stream = await client.subscribe(SESSION_ID);
    const step = await stream.next();
    assert.equal(step.kind, "closed");
    if (step.kind === "closed") {
      assert.equal(step.error.code, "local_daemon_connection_unavailable");
    }
  });
});

test("the retained window bounds the rows a long stream holds", async () => {
  await withDaemon(async (daemon, client) => {
    daemon.handler = (request, socket) => {
      daemon.reply(socket, request.id as number, {
        kind: "session_subscribed",
        data: snapshot([], null),
      });
      for (let index = 1; index <= RETAINED_TRANSCRIPT_MESSAGES + 4; index += 1) {
        daemon.frame(socket, { kind: "content", data: row(index, "user", `row ${index}`) });
      }
      // The daemon closes once the frames are written, so the read below ends
      // on the stream's own end instead of waiting for a frame that never comes.
      socket.end();
    };
    const stream = await client.subscribe(SESSION_ID);
    for (;;) {
      const step = await stream.next();
      if (step.kind === "closed") {
        break;
      }
    }
    assert.equal(stream.state.messages.length, RETAINED_TRANSCRIPT_MESSAGES);
    assert.equal(stream.state.messages[0]?.id, 5);
    stream.close();
  });
});

test("a daemon that is not listening is unavailable", async () => {
  const directory = await mkdtemp(join(tmpdir(), "intention-tui-"));
  const endpoint: LocalEndpoint = {
    instanceId: "intention-relay",
    address: join(directory, "absent.sock"),
  };
  const client = new IntentionClient(endpoint, {
    program: "intention-daemon",
    launch: () => undefined,
  });
  await assert.rejects(
    () => client.sessionSnapshot(SESSION_ID),
    (error: unknown) =>
      error instanceof ClientFailure && error.dto.code === "local_daemon_connection_unavailable",
  );
  await rm(directory, { recursive: true, force: true });
});

test("bootstrap starts one daemon and waits for its health", async () => {
  const directory = await mkdtemp(join(tmpdir(), "intention-tui-"));
  const daemon = new FakeDaemon(directory);
  const previous = process.env.XDG_RUNTIME_DIR;
  process.env.XDG_RUNTIME_DIR = directory;
  let launches = 0;
  daemon.handler = (_request, socket) => {
    daemon.reply(socket, 1, { kind: "daemon_health", data: {} });
  };
  const client = new IntentionClient(daemon.endpoint, {
    program: "intention-daemon",
    launch: () => {
      launches += 1;
      // A real launcher spawns a process; the stand-in starts listening once
      // the launch is observed, which is exactly what readiness polls for.
      void daemon.listen();
    },
  });
  try {
    await client.connectOrBootstrap();
    assert.equal(launches, 1);
  } finally {
    if (previous === undefined) {
      delete process.env.XDG_RUNTIME_DIR;
    } else {
      process.env.XDG_RUNTIME_DIR = previous;
    }
    await daemon.close();
  }
});

test("a bootstrap that never sees health reports the last failure", async () => {
  const directory = await mkdtemp(join(tmpdir(), "intention-tui-"));
  const endpoint: LocalEndpoint = {
    instanceId: "intention-relay",
    address: join(directory, "absent.sock"),
  };
  const previous = process.env.XDG_RUNTIME_DIR;
  process.env.XDG_RUNTIME_DIR = directory;
  const client = new IntentionClient(endpoint, {
    program: "intention-daemon",
    launch: () => undefined,
  });
  try {
    await assert.rejects(
      () => client.connectOrBootstrap(),
      (error: unknown) =>
        error instanceof ClientFailure && error.dto.code === "local_daemon_connection_unavailable",
    );
  } finally {
    if (previous === undefined) {
      delete process.env.XDG_RUNTIME_DIR;
    } else {
      process.env.XDG_RUNTIME_DIR = previous;
    }
    await rm(directory, { recursive: true, force: true });
  }
});
