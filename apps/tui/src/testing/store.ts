//! Stores a test drives without a daemon.
//!
//! Every helper builds a real store and feeds it the same facts the client feeds
//! the running app, so what a test asserts about a frame is what a reader sees.

import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ThemePreference } from "../app/action";
import { AppStore } from "../app/store";
import type { MessageProjectionDto, RunProjectionDto } from "../bindings";
import { IntentionClient } from "../client/client";
import { SessionStreamState } from "../client/stream";
import { delta, run, snapshot } from "./fixtures";

/** Builds one store over a client, with a recorder that writes nothing. */
export function storeFor(
  client: IntentionClient,
  options: { initialSession?: string | null; theme?: ThemePreference } = {},
): AppStore {
  return new AppStore({
    client,
    workspaceRoot: "/home/user/project",
    mode: null,
    initialSession: options.initialSession ?? null,
    theme: options.theme ?? "dark",
    recorder: { writeTheme: async () => undefined },
    onQuit: () => undefined,
  });
}

/** Builds one store over a client whose endpoint no daemon answers on. */
export function offlineStore(theme: ThemePreference = "dark"): AppStore {
  const client = new IntentionClient(
    { instanceId: "intention-relay", address: join(tmpdir(), "intention-relay-absent.sock") },
    {
      program: "intention-daemon",
      launch: () => {
        throw new Error("no daemon in this test");
      },
    },
  );
  return storeFor(client, { theme });
}

/** What a test hands a session when it opens one. */
export interface OpenSessionOptions {
  /** The committed rows the session starts with. */
  readonly messages?: MessageProjectionDto[];
  /** The run the session projects. */
  readonly run?: RunProjectionDto | null;
  /** The transient answer text the step in flight has produced. */
  readonly answer?: string;
  /** The transient reasoning text the step in flight has produced. */
  readonly reasoning?: string;
}

/**
 * Opens one live session on a store.
 *
 * The rows, the run, and the transient text are the client's own state, exactly
 * as a subscription hands it over, and the state is returned so a test can apply
 * the frames a daemon would send next.
 */
export function openSession(store: AppStore, options: OpenSessionOptions = {}): SessionStreamState {
  const asksForATail = options.answer !== undefined || options.reasoning !== undefined;
  // Transient text belongs to the session's current run, so a session asked for
  // a tail gets the one run that can carry it.
  const activeRun =
    options.run === undefined ? (asksForATail ? run("running") : null) : options.run;
  const state = SessionStreamState.fromSnapshot(snapshot(options.messages ?? [], activeRun));
  if (options.reasoning !== undefined) {
    state.applyFrame({ kind: "text_delta", data: delta(1, "reasoning", options.reasoning) });
  }
  if (options.answer !== undefined) {
    state.applyFrame({ kind: "text_delta", data: delta(1, "answer", options.answer) });
  }
  store.dispatch({ kind: "session_stream_opened", state });
  return state;
}
