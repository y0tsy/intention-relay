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
  SessionSummariesDto,
} from "../bindings";
import type { SessionStreamState } from "../client/stream";
import type { DraftEdit } from "./editor";

/** One fact the client established or one gesture the user made. */
export type Action =
  /** The front end asked for the daemon, at start or after a loss. */
  | { readonly kind: "connect_requested" }
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
  /**
   * The front end measured the live turn's own elapsed time.
   *
   * The clock lives in the front end, so the value arrives here as an action and
   * the state stays a pure function of what it was told.
   */
  | { readonly kind: "elapsed_reported"; readonly millis: number }
  /** The daemon accepted one stop request. */
  | { readonly kind: "run_cancel_accepted" }
  /** Stopping the active run failed. */
  | { readonly kind: "run_cancel_failed"; readonly error: ErrorDto }
  /** The user typed or pasted text into the input line. */
  | { readonly kind: "draft_typed"; readonly text: string }
  /** The user made one caret edit in the input line. */
  | { readonly kind: "draft_edited"; readonly edit: DraftEdit }
  /** The user asked for a line break in the input line. */
  | { readonly kind: "draft_newline" }
  /** The user cleared the input line. */
  | { readonly kind: "draft_cleared" }
  /** The user walked the submitted prompts. */
  | { readonly kind: "history_recalled"; readonly delta: number }
  /** The surface shows one notice line. */
  | { readonly kind: "notice_shown"; readonly text: string }
  /** The user submitted the input line. */
  | { readonly kind: "input_submitted" }
  /** The user asked to leave the front end. */
  | { readonly kind: "quit_requested" }
  /** The user asked for a new session. */
  | { readonly kind: "new_session_requested" }
  /** The user named one session to open. */
  | { readonly kind: "session_open_requested"; readonly sessionId: SessionId }
  /** The user asked to stop the active run for good. */
  | { readonly kind: "cancel_run_requested" }
  /** The user asked for the session list. */
  | { readonly kind: "sessions_requested" }
  /** The daemon listed the sessions it holds. */
  | { readonly kind: "sessions_listed"; readonly summaries: SessionSummariesDto }
  /** Listing the sessions failed. */
  | { readonly kind: "sessions_list_failed"; readonly error: ErrorDto }
  /** The user moved the selection of the surface that owns the keyboard. */
  | { readonly kind: "selection_moved"; readonly delta: number }
  /** The user committed the selection of the surface that owns the keyboard. */
  | { readonly kind: "selection_committed" }
  /** The user dismissed the surface that owns the keyboard. */
  | { readonly kind: "surface_closed" }
  /** The user asked for the theme picker. */
  | { readonly kind: "theme_requested" }
  /** The user chose a theme preference. */
  | { readonly kind: "theme_selected"; readonly preference: ThemePreference }
  /** An effect failed before it reached the daemon. */
  | { readonly kind: "effect_failed"; readonly error: ErrorDto };

/**
 * Which palette the terminal app paints with.
 *
 * `auto` follows the terminal's own light or dark background; the other two
 * pin it, and the choice is written back to the configuration file.
 */
export type ThemePreference = "auto" | "light" | "dark";

/**
 * Every theme preference, in the one order the app lists and indexes them.
 *
 * The picker's highlight is an index into this list, so the order is a
 * contract: a surface that listed the preferences differently would commit the
 * one next to the reader's choice. The command registry takes its vocabulary
 * from here too, so the words a line may name cannot drift from the picker.
 */
export const THEME_PREFERENCES: readonly ThemePreference[] = ["auto", "dark", "light"];

/** Returns the run identity a live run carries, for a caller that needs it. */
export type RunIdentity = RunId;
