//! The store: one reducer, one client, one live stream.
//!
//! A store owns the state, performs the effects the reducer asks for, and runs
//! the one session stream. Views read the store and dispatch actions; nothing
//! else in the app talks to the client, so every fact reaches the state through
//! the same door a test uses.

import { randomUUID } from "node:crypto";
import type { CreateSessionCommandDto, ErrorDto, RunModeDto, WorkspaceRootDto } from "../bindings";
import type { IntentionClient, SessionStream, StreamStep } from "../client/client";
import { ClientFailure } from "../client/errors";
import type { Action, ThemePreference } from "./action";
import type { Effect } from "./effect";
import { AppState } from "./state";

/** What a store needs to exist without a daemon. */
export interface StoreOptions {
  /** The client the store drives. */
  readonly client: IntentionClient;
  /** The workspace a created session is bound to. */
  readonly workspaceRoot: WorkspaceRootDto;
  /** The mode a created session runs in. */
  readonly mode: RunModeDto | null;
  /** The session to open on bootstrap, if any. */
  readonly initialSession: string | null;
  /** Where the theme preference is read from and written to. */
  readonly recorder: ThemeRecorder;
  /** The theme preference the configuration file declares. */
  readonly theme: ThemePreference;
  /** Leaves the front end, once a submitted line asks for it. */
  readonly onQuit: () => void;
}

/** Reads and writes the one theme preference the app persists. */
export interface ThemeRecorder {
  /** Writes one preference to the configuration file. */
  writeTheme(preference: ThemePreference): Promise<void>;
}

/** Reports how a store change happened, so a view can react to its own cause. */
export type StoreListener = () => void;

/**
 * One application store.
 *
 * The state is mutable and versioned: a view subscribes for "something changed"
 * and reads the state when it renders, which is exactly the contract
 * `useSyncExternalStore` needs.
 */
export class AppStore {
  readonly state: AppState;
  readonly #client: IntentionClient;
  readonly #workspaceRoot: WorkspaceRootDto;
  readonly #mode: RunModeDto | null;
  readonly #listeners = new Set<StoreListener>();
  readonly #recorder: ThemeRecorder;
  readonly #onQuit: () => void;
  #version = 0;
  #stream: SessionStream | null = null;
  #streamGeneration = 0;
  #disposed = false;

  constructor(options: StoreOptions) {
    this.#client = options.client;
    this.#workspaceRoot = options.workspaceRoot;
    this.#mode = options.mode;
    this.#recorder = options.recorder;
    this.#onQuit = options.onQuit;
    this.state = new AppState(options.initialSession, options.theme);
  }

  /** Subscribes one listener; returns the unsubscribe function. */
  subscribe(listener: StoreListener): () => void {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  }

  /** Returns the version a view compares to decide whether to re-render. */
  getVersion(): number {
    return this.#version;
  }

  /** Returns the workspace a session this store creates is bound to. */
  get workspaceRoot(): WorkspaceRootDto {
    return this.#workspaceRoot;
  }

  /** Dispatches one action and performs the effects it asks for. */
  dispatch(action: Action): void {
    if (this.#disposed) {
      return;
    }
    const effects = this.state.update(action);
    // A subscription the state no longer wants is a resource this store owns,
    // so it is closed before the next effect opens its replacement.
    if (action.kind === "session_stream_failed" || action.kind === "session_stream_ended") {
      this.closeStream();
    }
    this.#notify();
    for (const effect of effects) {
      void this.#perform(effect);
    }
  }

  /** Closes the live stream and stops delivering its frames. */
  closeStream(): void {
    this.#streamGeneration += 1;
    this.#stream?.close();
    this.#stream = null;
  }

  /** Releases every resource this store owns. */
  dispose(): void {
    this.#disposed = true;
    this.closeStream();
    this.#listeners.clear();
  }

  /** Performs one effect and feeds its outcome back as an action. */
  async #perform(effect: Effect): Promise<void> {
    switch (effect.kind) {
      case "connect":
        await this.#run(
          () => this.#client.connectOrBootstrap(),
          () => ({ kind: "bootstrapped" }),
          (error) => ({ kind: "bootstrap_failed", error }),
        );
        return;
      case "open_session":
        await this.#run(
          () => this.#client.sessionSnapshot(effect.sessionId),
          (snapshot) => ({ kind: "session_snapshot_loaded", snapshot }),
          (error) => ({ kind: "session_snapshot_failed", error }),
        );
        return;
      case "create_session":
        await this.#run(
          () => this.#createSession(effect.firstTurn),
          (sessionId) => ({ kind: "session_created", sessionId }),
          (error) => ({ kind: "session_create_failed", error }),
        );
        return;
      case "send_turn":
        await this.#run(
          () => this.#client.sendUserTurn(effect.sessionId, effect.content),
          (outcome) => ({ kind: "turn_accepted", outcome }),
          (error) => ({ kind: "turn_failed", error }),
        );
        return;
      case "cancel_run":
        await this.#run(
          () => this.#client.cancelRun(effect.sessionId, effect.runId),
          () => ({ kind: "run_cancel_accepted" }),
          (error) => ({ kind: "run_cancel_failed", error }),
        );
        return;
      case "subscribe":
        await this.#openStream(effect.sessionId);
        return;
      case "list_sessions":
        await this.#run(
          () => this.#client.listSessions(),
          (summaries) => ({ kind: "sessions_listed", summaries }),
          (error) => ({ kind: "sessions_list_failed", error }),
        );
        return;
      case "persist_theme":
        await this.#persistTheme(effect.preference);
        return;
      case "quit":
        this.dispose();
        this.#onQuit();
        return;
      default:
        assertNever(effect);
    }
  }

  /** Writes one theme preference to the configuration file. */
  async #persistTheme(preference: ThemePreference): Promise<void> {
    try {
      await this.#recorder.writeTheme(preference);
    } catch (error) {
      this.dispatch({ kind: "effect_failed", error: failureOf(error) });
    }
  }

  /** Creates one session bound to this store's workspace and mode. */
  async #createSession(firstTurn: string | null) {
    const command: CreateSessionCommandDto = {
      project_id: randomUUID(),
      session_id: randomUUID(),
      workspace_id: randomUUID(),
      workspace_root: this.#workspaceRoot,
      mode: this.#mode,
      first_turn: firstTurn === null ? null : { content: firstTurn },
    };
    return await this.#client.createSession(command);
  }

  /** Opens the session stream and pumps it until it ends. */
  async #openStream(sessionId: string): Promise<void> {
    this.closeStream();
    const generation = this.#streamGeneration;
    let stream: SessionStream;
    try {
      stream = await this.#client.subscribe(sessionId);
    } catch (error) {
      this.dispatch({ kind: "session_stream_failed", error: failureOf(error) });
      return;
    }
    if (generation !== this.#streamGeneration || this.#disposed) {
      stream.close();
      return;
    }
    this.#stream = stream;
    this.dispatch({ kind: "session_stream_opened", state: stream.state });
    void this.#pump(stream, generation);
  }

  /** Reads frames until the daemon closes the stream this store keeps. */
  async #pump(stream: SessionStream, generation: number): Promise<void> {
    for (;;) {
      let step: StreamStep;
      try {
        step = await stream.next();
      } catch (error) {
        if (generation === this.#streamGeneration && !this.#disposed) {
          this.dispatch({ kind: "session_stream_failed", error: failureOf(error) });
        }
        return;
      }
      if (generation !== this.#streamGeneration || this.#disposed) {
        return;
      }
      if (step.kind === "closed") {
        this.dispatch({ kind: "session_stream_ended" });
        return;
      }
      this.dispatch({ kind: "frame_received", state: step.state });
    }
  }

  /** Runs one client call and turns its outcome into the matching action. */
  async #run<T>(
    operation: () => Promise<T>,
    onSuccess: (value: T) => Action,
    onFailure: (error: ErrorDto) => Action,
  ): Promise<void> {
    let action: Action;
    try {
      action = onSuccess(await operation());
    } catch (error) {
      action = onFailure(failureOf(error));
    }
    this.dispatch(action);
  }

  /** Tells every listener that the state moved. */
  #notify(): void {
    this.#version += 1;
    for (const listener of this.#listeners) {
      listener();
    }
  }
}

/** Makes an unhandled effect a compile error instead of a silent no-op. */
function assertNever(effect: never): never {
  throw new Error(`unhandled effect: ${JSON.stringify(effect)}`);
}

/** Reads the typed failure out of anything a client call threw. */
function failureOf(error: unknown): ErrorDto {
  if (error instanceof ClientFailure) {
    return error.dto;
  }
  return {
    code: "terminal_app_failure",
    category: "internal",
    message: error instanceof Error ? error.message : "the terminal app failed",
    retryable: false,
  };
}
