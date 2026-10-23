//! The client this app drives: one request per connection, one live stream.
//!
//! Every request opens a fresh connection and reads exactly one correlated
//! answer, so the daemon never has to demultiplex. A session stream is the
//! exception: it opens one connection, subscribes, and then reads committed and
//! transient frames until the daemon closes it.

import type {
  CancelRunCommandDto,
  ClientRequestDto,
  CreateSessionCommandDto,
  DaemonHealthDto,
  ErrorDto,
  GetSessionSnapshotQueryDto,
  ProtocolDaemonMessageDto,
  ProtocolResultDto,
  RunCancelledAcceptedDto,
  SendUserTurnOutcomeDto,
  SessionId,
  SessionSnapshotDto,
  SessionSummariesDto,
  SubscribeSessionCommandDto,
} from "../bindings";
import { Connection, withDeadline } from "./connection";
import type { DaemonLauncher } from "./daemon";
import {
  acquireStartupLock,
  STARTUP_RETRY_MILLISECONDS,
  STARTUP_TIMEOUT_MILLISECONDS,
  sleep,
} from "./daemon";
import type { LocalEndpoint } from "./endpoint";
import {
  ClientFailure,
  invalidResponse,
  isUnavailable,
  requestTimeout,
  streamReplyTimeout,
  unavailable,
} from "./errors";
import { decodeDaemonMessage, encodeRequest, REQUEST_ID, subscribedSnapshot } from "./protocol";
import { type FrameUpdate, SessionStreamState } from "./stream";

/** How long one request may take before the client reports a timeout. */
export const REQUEST_TIMEOUT_MILLISECONDS = 10_000;

/** How long a subscribe reply may take before the client reports a timeout. */
export const STREAM_REPLY_TIMEOUT_MILLISECONDS = 30_000;

/** One observation of a live session stream. */
export type StreamStep =
  | { readonly kind: "frame"; readonly update: FrameUpdate; readonly state: SessionStreamState }
  | { readonly kind: "closed"; readonly error: ErrorDto };

/** The client for one local endpoint and its launcher seam. */
export class IntentionClient {
  readonly #endpoint: LocalEndpoint;
  readonly #launcher: DaemonLauncher;

  constructor(endpoint: LocalEndpoint, launcher: DaemonLauncher) {
    this.#endpoint = endpoint;
    this.#launcher = launcher;
  }

  /** Returns the endpoint this client addresses. */
  get endpoint(): LocalEndpoint {
    return this.#endpoint;
  }

  /**
   * Connects to a ready daemon, launching exactly one when none is listening.
   *
   * The client tries the connection before spawning, and holds the per-endpoint
   * launch lock only while it starts a daemon it established was absent.
   *
   * @throws {ClientFailure} The last typed failure when no daemon answers
   * within the bootstrap budget.
   */
  async connectOrBootstrap(): Promise<DaemonHealthDto> {
    const existing = await this.#probeHealth();
    if (existing !== null) {
      return existing;
    }
    const lock = await acquireStartupLock(this.#endpoint);
    try {
      const raced = await this.#probeHealth();
      if (raced !== null) {
        return raced;
      }
      this.#launcher.launch(this.#endpoint);
      return await this.#waitForReady();
    } finally {
      await lock.release();
    }
  }

  /**
   * Creates one durable session.
   *
   * @throws {ClientFailure} The daemon's typed rejection or a transport failure.
   */
  async createSession(command: CreateSessionCommandDto): Promise<SessionSnapshotDto["session_id"]> {
    const result = await this.#request({ kind: "create_session", data: command });
    if (result.kind !== "session_created") {
      throw new ClientFailure(invalidResponse());
    }
    return result.data.session_id;
  }

  /**
   * Sends one user turn and returns the daemon's durable turn outcome.
   *
   * @throws {ClientFailure} The daemon's typed rejection or a transport failure.
   */
  async sendUserTurn(sessionId: SessionId, content: string): Promise<SendUserTurnOutcomeDto> {
    const result = await this.#request({
      kind: "send_user_turn",
      data: { session_id: sessionId, content },
    });
    if (result.kind !== "turn_accepted") {
      throw new ClientFailure(invalidResponse());
    }
    return result.data.outcome;
  }

  /**
   * Requests that one exact active run stop for good.
   *
   * @throws {ClientFailure} The daemon's typed rejection or a transport failure.
   */
  async cancelRun(
    sessionId: SessionId,
    runId: RunCancelledAcceptedDto["run_id"],
  ): Promise<RunCancelledAcceptedDto> {
    const command: CancelRunCommandDto = { session_id: sessionId, run_id: runId };
    const result = await this.#request({ kind: "cancel_run", data: command });
    if (result.kind !== "run_cancelled") {
      throw new ClientFailure(invalidResponse());
    }
    return result.data;
  }

  /**
   * Lists the current durable sessions in their durable order.
   *
   * The reply carries an explicit omitted count, so a caller renders the window
   * as partial instead of reading it as the complete list.
   *
   * @throws {ClientFailure} The daemon's typed rejection or a transport failure.
   */
  async listSessions(): Promise<SessionSummariesDto> {
    const result = await this.#request({ kind: "list_sessions" });
    if (result.kind !== "sessions_listed") {
      throw new ClientFailure(invalidResponse());
    }
    return result.data;
  }

  /**
   * Reads the current snapshot of one session.
   *
   * @throws {ClientFailure} The daemon's typed rejection or a transport failure.
   */
  async sessionSnapshot(sessionId: SessionId): Promise<SessionSnapshotDto> {
    const query: GetSessionSnapshotQueryDto = { session_id: sessionId };
    const result = await this.#request({ kind: "get_session_snapshot", data: query });
    if (result.kind !== "session_snapshot") {
      throw new ClientFailure(invalidResponse());
    }
    return result.data;
  }

  /**
   * Opens one live session stream.
   *
   * The reply is the current snapshot; every later frame - committed row, run
   * status, or transient delta - arrives on the same connection.
   *
   * @throws {ClientFailure} The daemon's typed rejection, a transport failure,
   * or a timeout when the reply does not arrive.
   */
  async subscribe(sessionId: SessionId): Promise<SessionStream> {
    const command: SubscribeSessionCommandDto = { session_id: sessionId };
    const connection = await Connection.open(this.#endpoint);
    try {
      await connection.write(
        encodeRequest(REQUEST_ID, { kind: "subscribe_session", data: command }),
      );
      const line = await withDeadline(
        () => connection.readLine(),
        STREAM_REPLY_TIMEOUT_MILLISECONDS,
        streamReplyTimeout,
      );
      if (line === null) {
        throw new ClientFailure(unavailable());
      }
      const message = decodeDaemonMessage(line);
      const snapshot = subscribedSnapshot(this.#replyResult(message));
      return new SessionStream(connection, SessionStreamState.fromSnapshot(snapshot));
    } catch (error) {
      connection.close();
      throw error;
    }
  }

  /** Reads one health reply, or reports `null` when no daemon is listening. */
  async #probeHealth(): Promise<DaemonHealthDto | null> {
    try {
      const result = await this.#request({ kind: "get_daemon_health" });
      if (result.kind !== "daemon_health") {
        throw new ClientFailure(invalidResponse());
      }
      return result.data;
    } catch (error) {
      if (error instanceof ClientFailure && isUnavailable(error.dto)) {
        return null;
      }
      throw error;
    }
  }

  /** Polls the endpoint until the daemon reports ready health, or the budget ends. */
  async #waitForReady(): Promise<DaemonHealthDto> {
    const deadline = Date.now() + STARTUP_TIMEOUT_MILLISECONDS;
    let last: ErrorDto = unavailable();
    for (;;) {
      try {
        const result = await this.#request({ kind: "get_daemon_health" });
        if (result.kind === "daemon_health") {
          return result.data;
        }
        last = invalidResponse();
      } catch (error) {
        if (!(error instanceof ClientFailure)) {
          throw error;
        }
        last = error.dto;
      }
      if (Date.now() >= deadline) {
        throw new ClientFailure(last);
      }
      await sleep(STARTUP_RETRY_MILLISECONDS);
    }
  }

  /** Sends one request on a fresh connection and decodes its correlated reply. */
  async #request(request: ClientRequestDto): Promise<ProtocolResultDto> {
    const connection = await Connection.open(this.#endpoint);
    try {
      const message = await withDeadline(
        async () => {
          await connection.write(encodeRequest(REQUEST_ID, request));
          return await connection.readLine();
        },
        REQUEST_TIMEOUT_MILLISECONDS,
        requestTimeout,
      );
      if (message === null) {
        throw new ClientFailure(unavailable());
      }
      return this.#replyResult(decodeDaemonMessage(message));
    } finally {
      connection.close();
    }
  }

  /** Reads the result out of one daemon reply, turning a rejection into a failure. */
  #replyResult(message: ProtocolDaemonMessageDto): ProtocolResultDto {
    switch (message.kind) {
      case "reply":
        if (message.data.id !== REQUEST_ID) {
          throw new ClientFailure(invalidResponse());
        }
        return message.data.result;
      case "rejection":
        throw new ClientFailure(message.data.error);
      case "frame":
        throw new ClientFailure(invalidResponse());
    }
  }
}

/**
 * One live session stream with the state it has committed so far.
 *
 * The stream owns the derivation: every frame is applied to one state, and the
 * caller reads that state instead of replaying frames into a copy of its own.
 */
export class SessionStream {
  readonly #connection: Connection;
  #state: SessionStreamState;
  #ended = false;

  constructor(connection: Connection, state: SessionStreamState) {
    this.#connection = connection;
    this.#state = state;
  }

  /** Returns the committed state of this fixed session scope. */
  get state(): SessionStreamState {
    return this.#state;
  }

  /**
   * Reads, applies, and returns the next observation of this stream.
   *
   * A closed connection is reported as a `closed` step, which is the client's
   * own transport classification rather than an error. Live frames may be
   * arbitrarily sparse, so this wait carries no deadline.
   *
   * @throws {ClientFailure} The typed framing or scope failure for a frame this
   * state cannot accept.
   */
  async next(): Promise<StreamStep> {
    if (this.#ended) {
      throw new ClientFailure(unavailable());
    }
    const line = await this.#connection.readLine();
    if (line === null) {
      this.close();
      return { kind: "closed", error: unavailable() };
    }
    const message = decodeDaemonMessage(line);
    if (message.kind !== "frame") {
      throw new ClientFailure(invalidResponse());
    }
    const update = this.#state.applyFrame(message.data);
    return { kind: "frame", update, state: this.#state };
  }

  /** Ends this observation and closes the connection. */
  close(): void {
    this.#ended = true;
    this.#connection.close();
  }
}
