//! The actions one front end feeds into the reducer.
//!
//! An action is either a fact the client established or a gesture the user
//! made. Both enter through the same door, so a transition is written once and
//! a test drives it exactly as the running app does.

import type {
  ErrorDto,
  RunId,
  SendUserTurnOutcomeDto,
  SessionId,
  SessionSnapshotDto,
} from "../bindings";
import type { SessionStreamState } from "../client/stream";

/** One fact the client established or one gesture the user made. */
export type Action =
  /** The client reached a ready daemon. */
  | { readonly kind: "bootstrapped" }
  /** The client could not reach a daemon. */
  | { readonly kind: "bootstrap_failed"; readonly error: ErrorDto }
  /** One session snapshot arrived. */
  | { readonly kind: "session_snapshot_loaded"; readonly snapshot: SessionSnapshotDto }
  /** Reading one session snapshot failed. */
  | { readonly kind: "session_snapshot_failed"; readonly error: ErrorDto }
  /** The daemon created one session. */
  | { readonly kind: "session_created"; readonly sessionId: SessionId }
  /** Creating one session failed. */
  | { readonly kind: "session_create_failed"; readonly error: ErrorDto }
  /** A session stream opened with the client's committed state. */
  | { readonly kind: "session_stream_opened"; readonly state: SessionStreamState }
  /** Opening or keeping a session stream failed. */
  | { readonly kind: "session_stream_failed"; readonly error: ErrorDto }
  /** The daemon closed the session stream. */
  | { readonly kind: "session_stream_ended" }
  /** One stream frame arrived, already derived by the client. */
  | { readonly kind: "frame_received"; readonly state: SessionStreamState }
  /** The daemon accepted one user turn. */
  | { readonly kind: "turn_accepted"; readonly outcome: SendUserTurnOutcomeDto }
  /** Sending one user turn failed. */
  | { readonly kind: "turn_failed"; readonly error: ErrorDto }
  /** The daemon accepted one stop request. */
  | { readonly kind: "run_cancel_accepted" }
  /** Stopping the active run failed. */
  | { readonly kind: "run_cancel_failed"; readonly error: ErrorDto }
  /** The user submitted one prompt as a turn. */
  | { readonly kind: "turn_submitted"; readonly content: string }
  /** The user asked for a new session. */
  | { readonly kind: "new_session_requested" }
  /** The user asked to stop the active run for good. */
  | { readonly kind: "cancel_run_requested" }
  /** An effect failed before it reached the daemon. */
  | { readonly kind: "effect_failed"; readonly error: ErrorDto };

/** Returns the run identity a live run carries, for a caller that needs it. */
export type RunIdentity = RunId;
