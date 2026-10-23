//! The values a view subscribes to.
//!
//! Every selector is a pure read of the state, and every one of them returns
//! either a primitive or an identity the state already owns. That rule is what
//! makes a view repaint only when the value it read actually moved: a selector
//! that built a fresh array or object would hand React a new value on every
//! call, and the subscription would repaint forever. The state's own lists - the
//! committed rows, the session summaries - are the identities to return.

import type {
  MessageProjectionDto,
  RunModeDto,
  RunProjectionDto,
  RunStatusDto,
  SessionSummariesDto,
} from "../bindings";
import type { ThemePreference } from "./action";
import type { Draft } from "./editor";
import { type CommandMenu, commandMenu } from "./menu";
import type { RunPhase } from "./run";
import type { AppState, ConnectionStatus, Surface } from "./state";

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

/**
 * Returns the rows the word the caret sits in opens, or `null` while no band is
 * open.
 *
 * The menu is derived, and the binding reads it once per store version, so a
 * keystroke that does not move the word costs no band.
 */
export function selectCommandMenu(state: AppState): CommandMenu | null {
  return commandMenu(state.draft);
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

/** Returns whether the daemon this front end talks to has answered. */
export function selectConnection(state: AppState): ConnectionStatus {
  return state.connection;
}

/** Returns the run mode the open session's projection carries. */
export function selectSessionMode(state: AppState): RunModeDto | null {
  return state.sessionMode;
}

/** Returns the effective provider model the session's projection names. */
export function selectSessionModel(state: AppState): string | null {
  return state.sessionModel;
}

/** Returns the live phase of the run this front end observes. */
export function selectRunPhase(state: AppState): RunPhase | null {
  return state.runPhase;
}

/** Returns the elapsed time this front end measured for the current turn. */
export function selectElapsed(state: AppState): number | null {
  return state.elapsed;
}

/** Returns the chunks each reasoning block has revealed, keyed by block identity. */
export function selectExpansions(state: AppState): ReadonlyMap<string, number> {
  return state.expansions;
}

/** Returns when the turn in flight began, in Unix milliseconds. */
export function selectTurnStartedAt(state: AppState): number | null {
  return state.turnStart;
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

/** Returns the committed usage the active run's projection carries. */
export function selectUsage(
  state: AppState,
): "not_reported" | { readonly input: number; readonly output: number } | null {
  const usage = state.activeRun?.usage;
  if (usage === undefined || usage === null) {
    return null;
  }
  return usage.state === "reported"
    ? { input: usage.input_tokens, output: usage.output_tokens }
    : "not_reported";
}

/** Returns when the last known run finished, in whole Unix seconds. */
export function selectFinishedAt(state: AppState): number | null {
  return state.activeRun?.finished_at ?? null;
}

/** Returns the tokens the last known run reports, or `null` when it reports none. */
export function selectTokens(state: AppState): number | null {
  const usage = state.activeRun?.usage;
  return usage?.state === "reported" ? usage.total_tokens : null;
}
