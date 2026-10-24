//! A daemon stand-in that answers whatever a test tells it to.
//!
//! The stand-in speaks the real framing over a real local socket, so a test
//! exercises the framing, the correlation, and the stream loop as they run
//! instead of against a mock of them.

import { mkdtemp, rm } from "node:fs/promises";
import { createServer, type Server, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { IntentionClient } from "../client/client";
import type { LocalEndpoint } from "../client/endpoint";

/** One daemon stand-in that answers whatever a test tells it to. */
export class FakeDaemon {
  readonly server: Server;
  readonly sockets: Socket[] = [];
  readonly requests: Array<Record<string, unknown>> = [];
  handler: (request: Record<string, unknown>, socket: Socket) => void = () => undefined;
  readonly #directory: string;
  readonly endpoint: LocalEndpoint;

  constructor(directory: string) {
    this.#directory = directory;
    const address = join(directory, "intention-relay.sock");
    this.endpoint = { instanceId: "intention-relay", address };
    this.server = createServer((socket) => this.#accept(socket));
  }

  /** Starts listening on the fixture's socket. */
  async listen(): Promise<void> {
    const address = this.endpoint.address;
    await new Promise<void>((resolve) => {
      this.server.listen(address, resolve);
    });
  }

  /** Stops listening and drops every open connection. */
  async close(): Promise<void> {
    for (const socket of this.sockets.splice(0)) {
      socket.destroy();
    }
    await new Promise<void>((resolve) => {
      this.server.close(() => resolve());
    });
    await rm(this.#directory, { recursive: true, force: true });
  }

  /** Replies to the request it just read. */
  reply(socket: Socket, id: number, result: unknown): void {
    socket.write(`${JSON.stringify({ kind: "reply", data: { id, result } })}\n`);
  }

  /** Rejects the request it just read. */
  reject(socket: Socket, id: number | null, code: string): void {
    socket.write(
      `${JSON.stringify({
        kind: "rejection",
        data: {
          id,
          error: { code, category: "validation", message: "the daemon said no", retryable: false },
        },
      })}\n`,
    );
  }

  /** Sends one uncorrelated frame. */
  frame(socket: Socket, data: unknown): void {
    socket.write(`${JSON.stringify({ kind: "frame", data })}\n`);
  }

  #accept(socket: Socket): void {
    this.sockets.push(socket);
    let buffer = "";
    socket.on("data", (chunk: string) => {
      buffer += chunk;
      let boundary = buffer.indexOf("\n");
      while (boundary >= 0) {
        const line = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 1);
        if (line.length > 0) {
          const request = JSON.parse(line) as Record<string, unknown>;
          this.requests.push(request);
          this.handler(request, socket);
        }
        boundary = buffer.indexOf("\n");
      }
    });
    socket.on("error", () => undefined);
  }
}

/** Runs one test against a fresh stand-in daemon. */
export async function withDaemon(
  body: (daemon: FakeDaemon, client: IntentionClient) => Promise<void>,
): Promise<void> {
  const directory = await mkdtemp(join(tmpdir(), "intention-tui-"));
  const daemon = new FakeDaemon(directory);
  await daemon.listen();
  // The launch lock lives beside the endpoint, so the test roots it in the
  // fixture's own directory instead of the machine's runtime directory.
  const previous = process.env.XDG_RUNTIME_DIR;
  process.env.XDG_RUNTIME_DIR = directory;
  const client = new IntentionClient(daemon.endpoint, {
    program: "intention-daemon",
    launch: () => {
      throw new Error("no launch expected");
    },
  });
  try {
    await body(daemon, client);
  } finally {
    if (previous === undefined) {
      delete process.env.XDG_RUNTIME_DIR;
    } else {
      process.env.XDG_RUNTIME_DIR = previous;
    }
    await daemon.close();
  }
}
