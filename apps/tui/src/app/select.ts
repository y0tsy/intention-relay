//! The values a view subscribes to.
//!
//! Every selector is a pure read of the state, and every one of them returns
//! either a primitive or an identity the state already owns. That rule is what
//! makes a view repaint only when the value it read actually moved: a selector
//! that built a fresh array or object would hand React a new value on every
//! call, and the subscription would repaint forever. The state's own lists - the
//! committed rows, the session summaries - are the identities to return.

import type { MessageProjectionDto, SessionSummariesDto } from "../bindings";
import type { RunProjectionDto, RunStatusDto } from "../bindings";
import type { AppState, Surface } from "./state";
import type { ThemePreference } from "./action";
import type { Draft } from "./editor";

/** Returns the committed rows of the open session. */
export function selectTranscript(state: AppState): readonly MessageProjectionDto[] {
  return state.transcript;
}

/** Returns the transient answer text of the step in flight. */
export function selectProvisionalText(state: AppState): string {
  return state.provisionalText;
}

/** Returns the transient reasoning text of the step in flight. */
export function selectProvisionalReasoning(state: AppState): string {
  return state.provisionalReasoning;
}

/** Returns the run this front end observes as live, if any. */
export function selectLiveRun(state: AppState): RunProjectionDto | null {
  return state.liveRun;
}

/** Returns the last known run lifecycle status. */
export function selectRunStatus(state: AppState): RunStatusDto | null {
  return state.runStatus;
}

/** Returns the last known run projection. */
export function selectActiveRun(state: AppState): RunProjectionDto | null {
  return state.activeRun;
}

/** Returns whether the transcript has nothing to show yet. */
export function selectEmpty(state: AppState): boolean {
  return state.transcript.length === 0 && state.provisionalText.length === 0;
}

/** Returns the surface that owns the keyboard. */
export function selectSurface(state: AppState): Surface {
  return state.surface;
}

/** Returns the failure the chat surface reports. */
export function selectError(state: AppState): string | null {
  return state.error;
}

/** Returns the notice the chat surface reports. */
export function selectNotice(state: AppState): string | null {
  return state.notice;
}

/** Returns the input line the composer shows. */
export function selectDraft(state: AppState): Draft {
  return state.draft;
}

/** Returns the session summaries the browser shows, once the daemon answered. */
export function selectSessions(state: AppState): SessionSummariesDto | null {
  return state.sessions;
}

/** Returns the failure the session browser reports instead of a list. */
export function selectSessionsError(state: AppState): string | null {
  return state.sessionsError;
}

/** Returns the index the surface that owns the keyboard has highlighted. */
export function selectSelection(state: AppState): number {
  return state.selection;
}

/** Returns the palette preference in force. */
export function selectTheme(state: AppState): ThemePreference {
  return state.theme;
}

/** Returns the open session's identity, or `null` before one is open. */
export function selectSessionId(state: AppState): string | null {
  return state.sessionId;
}

/** Returns whether a run this front end observes is still live. */
export function selectLive(state: AppState): boolean {
  return state.liveRun !== null;
}

/** Returns whether the step in flight has produced answer text. */
export function selectAnswering(state: AppState): boolean {
  return state.provisionalText.length > 0;
}

/** Returns when the last known run started, in whole Unix seconds. */
export function selectStartedAt(state: AppState): number | null {
  return state.activeRun?.started_at ?? null;
}

/** Returns the tokens the last known run reports, or `null` when it reports none. */
export function selectTokens(state: AppState): number | null {
  const usage = state.activeRun?.usage;
  return usage?.state === "reported" ? usage.total_tokens : null;
}
