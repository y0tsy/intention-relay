//! One framed connection to the local daemon.
//!
//! A connection carries a single line in each direction at a time: the client
//! writes one request and reads the one reply that answers it, or it subscribes
//! and reads the frames the daemon sends until the daemon closes. The framing is
//! the transport's: one JSON document per newline-terminated line, bounded at
//! [`MAX_MESSAGE_BYTES`].

import { connect, type Socket } from "node:net";
import type { ErrorDto } from "../bindings";
import type { LocalEndpoint } from "./endpoint";
import { ClientFailure, invalidLine, unavailable } from "./errors";
import { MAX_MESSAGE_BYTES } from "./protocol";

type Waiter = {
  resolve: (line: string | null) => void;
  reject: (failure: ClientFailure) => void;
};

/**
 * One established connection whose lines arrive in order.
 *
 * A line the client cannot accept - one longer than the envelope cap - ends the
 * connection and rejects the pending read: the daemon never sends one, so the
 * only alternative is a buffer grown on bytes this client cannot trust.
 */
export class Connection {
  readonly #socket: Socket;
  #residual = "";
  #lines: string[] = [];
  #waiters: Waiter[] = [];
  #failure: ErrorDto | null = null;
  #closed = false;

  private constructor(socket: Socket) {
    this.#socket = socket;
    socket.setEncoding("utf8");
    socket.on("data", (chunk: string) => this.#receive(chunk));
    socket.on("error", () => this.#finish());
    socket.on("close", () => this.#finish());
  }

  /**
   * Dials one endpoint.
   *
   * @throws {ClientFailure} The typed unavailable failure when the daemon is
   * not listening.
   */
  static async open(endpoint: LocalEndpoint): Promise<Connection> {
    const socket = connect({ path: endpoint.address });
    await new Promise<void>((resolve, reject) => {
      const onError = (): void => {
        socket.destroy();
        reject(new ClientFailure(unavailable()));
      };
      socket.once("error", onError);
      socket.once("connect", () => {
        socket.off("error", onError);
        resolve();
      });
    });
    return new Connection(socket);
  }

  /**
   * Writes one message as one framed line.
   *
   * @throws {ClientFailure} The typed transport failure when the write does not
   * reach the daemon.
   */
  async write(message: unknown): Promise<void> {
    const line = `${JSON.stringify(message)}\n`;
    if (line.length > MAX_MESSAGE_BYTES) {
      throw new ClientFailure(invalidLine());
    }
    await new Promise<void>((resolve, reject) => {
      this.#socket.write(line, (error) => {
        if (error) {
          reject(new ClientFailure(unavailable()));
          return;
        }
        resolve();
      });
    });
  }

  /**
   * Reads the next line, or `null` once the peer has closed the connection.
   *
   * @throws {ClientFailure} The typed framing failure for a line the envelope
   * cap cannot hold.
   */
  async readLine(): Promise<string | null> {
    const buffered = this.#lines.shift();
    if (buffered !== undefined) {
      return buffered;
    }
    if (this.#failure !== null) {
      throw new ClientFailure(this.#failure);
    }
    if (this.#closed) {
      return null;
    }
    return new Promise<string | null>((resolve, reject) => {
      this.#waiters.push({ resolve, reject });
    });
  }

  /** Closes this connection; a closed connection delivers no further lines. */
  close(): void {
    this.#socket.destroy();
    this.#finish();
  }

  /** Appends one received chunk to the frame buffer. */
  #receive(chunk: string): void {
    this.#residual += chunk;
    if (this.#residual.length > MAX_MESSAGE_BYTES) {
      this.#failure = invalidLine();
      this.close();
      return;
    }
    let boundary = this.#residual.indexOf("\n");
    while (boundary >= 0) {
      const line = this.#residual.slice(0, boundary);
      this.#residual = this.#residual.slice(boundary + 1);
      if (line.length > 0) {
        this.#deliver(line);
      }
      boundary = this.#residual.indexOf("\n");
    }
  }

  /** Hands one line to the oldest waiter, or queues it. */
  #deliver(line: string): void {
    const waiter = this.#waiters.shift();
    if (waiter === undefined) {
      this.#lines.push(line);
      return;
    }
    waiter.resolve(line);
  }

  /** Marks the connection closed and releases every waiter. */
  #finish(): void {
    if (this.#closed) {
      return;
    }
    this.#closed = true;
    const failure = this.#failure;
    for (const waiter of this.#waiters.splice(0)) {
      if (failure === null) {
        waiter.resolve(null);
        continue;
      }
      waiter.reject(new ClientFailure(failure));
    }
  }
}

/**
 * Runs one operation under a deadline.
 *
 * @throws The failure `onExpiry` builds when the deadline passes first.
 */
export async function withDeadline<T>(
  operation: () => Promise<T>,
  milliseconds: number,
  onExpiry: () => ErrorDto,
): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const deadline = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(() => reject(new ClientFailure(onExpiry())), milliseconds);
  });
  try {
    return await Promise.race([operation(), deadline]);
  } finally {
    clearTimeout(timer);
  }
}
