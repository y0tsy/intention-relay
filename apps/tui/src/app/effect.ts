//! The client operations the reducer asks for.
//!
//! An effect names work, never a client: the store decides how to run it, so the
//! reducer stays constructible, and testable, without a daemon.

import type { RunId, SessionId } from "../bindings";
import type { ThemePreference } from "./action";

/** One client operation the reducer asks the store to perform. */
export type Effect =
  /** Bootstrap or re-bootstrap the shared local client. */
  | { readonly kind: "connect" }
  /** Read the current snapshot of one session. */
  | { readonly kind: "open_session"; readonly sessionId: SessionId }
  /** Create a session from the front end's workspace and mode. */
  | {
      readonly kind: "create_session";
      /** The prompt the creation commits as the session's first turn, if any. */
      readonly firstTurn: string | null;
    }
  /** Send one user turn. */
  | { readonly kind: "send_turn"; readonly sessionId: SessionId; readonly content: string }
  /** Open the live stream of one session. */
  | { readonly kind: "subscribe"; readonly sessionId: SessionId }
  /** Stop one exact active run. */
  | { readonly kind: "cancel_run"; readonly sessionId: SessionId; readonly runId: RunId }
  /** Read the current durable session list. */
  | { readonly kind: "list_sessions" }
  /** Write one theme preference to the configuration file. */
  | { readonly kind: "persist_theme"; readonly preference: ThemePreference }
  /** Leave the front end. */
  | { readonly kind: "quit" };
