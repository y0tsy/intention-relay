//! The status row and the detail line.
//!
//! The row names what a reader acts on, in words: the connection, the session,
//! the session's run mode, the run's live phase, and the committed token usage.
//! The live phase is `Waiting…` from the moment a turn request leaves,
//! `Thinking…` when the reasoning channel starts, `Answering…` when the answer
//! channel does, and `Working…` while a tool call has been open past the
//! threshold. A completed run reads `Answered in 13s` from the projection's own
//! times, the interrupted and failed runs keep their words, and a live elapsed
//! value reads in tenths while the committed one reads in whole seconds. A run
//! that has committed no usage carries no usage segment rather than a fabricated
//! number.
//!
//! The detail line under the row is the failure, the notice, or the idle hint -
//! never a placeholder for a fact the front end does not carry.

import { committedSeconds, elapsedTenths, phaseWord, type RunPhase } from "../app/run";
import {
  selectConnection,
  selectElapsed,
  selectError,
  selectFinishedAt,
  selectNotice,
  selectRunPhase,
  selectRunStatus,
  selectSessionId,
  selectSessionMode,
  selectStartedAt,
  selectTurnStartedAt,
  selectUsage,
} from "../app/select";
import type { ConnectionStatus } from "../app/state";
import type { AppStore } from "../app/store";
import type { RunModeDto, RunStatusDto } from "../bindings";
import { Runs } from "../kit";
import type { Palette } from "../theme/palette";
import { SEPARATOR } from "./glyphs";
import { useRunClock } from "./run-clock";
import { useStore } from "./use-store";

/** The detail line this front end shows while no error or notice claims it. */
export const DEFAULT_HINT = "enter sends | /new session | /sessions switch";

/** The word for a daemon this front end has not reached yet. */
const CONNECTING = "Connecting…";

/** The word for a ready daemon. */
const READY = "Ready";

/** The word for a daemon the last attempt could not reach. */
const OFFLINE = "Offline";

/** One styled run of the status row. */
export interface StatusRun {
  /** The text the run paints. */
  readonly text: string;
  /** The role the run's ink comes from. */
  readonly ink: keyof Palette;
}

/** What the status row reads. */
export interface StatusFacts {
  /** Whether the daemon has answered. */
  readonly connection: ConnectionStatus;
  /** The open session's identity, or `null`. */
  readonly sessionId: string | null;
  /** The session's run mode, or `null`. */
  readonly mode: RunModeDto | null;
  /** The live phase, or `null` when no run is live. */
  readonly phase: RunPhase | null;
  /** The run's lifecycle status, or `null`. */
  readonly runStatus: RunStatusDto | null;
  /** The measured elapsed value of the live turn, in milliseconds. */
  readonly elapsed: number | null;
  /** The committed usage the projection carries. */
  readonly usage: "not_reported" | { readonly input: number; readonly output: number } | null;
  /** When the run started, in whole Unix seconds. */
  readonly startedAt: number | null;
  /** When the run finished, in whole Unix seconds. */
  readonly finishedAt: number | null;
}

/** Returns the eight-character identity a status row prints. */
export function shortIdentifier(id: string): string {
  return id.length <= 8 ? id : id.slice(0, 8);
}

/** Returns the connection word and the role it paints. */
function connectionRun(connection: ConnectionStatus): StatusRun {
  switch (connection) {
    case "connecting":
      return { text: CONNECTING, ink: "warning" };
    case "ready":
      return { text: READY, ink: "success" };
    case "offline":
      return { text: OFFLINE, ink: "error" };
  }
}

/** Returns the phrase one run reads as, or `null` when no run is named. */
function runPhrase(facts: StatusFacts): StatusRun[] | null {
  if (facts.phase !== null) {
    const runs: StatusRun[] = [{ text: phaseWord(facts.phase), ink: "scarlet" }];
    if (facts.elapsed !== null) {
      runs.push({ text: ` ${elapsedTenths(facts.elapsed)}`, ink: "timerInk" });
    }
    return runs;
  }
  switch (facts.runStatus) {
    case null:
      return null;
    case "completed": {
      const runs: StatusRun[] = [{ text: "Answered", ink: "success" }];
      if (facts.startedAt !== null && facts.finishedAt !== null) {
        runs.push({ text: " in ", ink: "success" });
        runs.push({
          text: `${committedSeconds(facts.startedAt, facts.finishedAt)}s`,
          ink: "timerInk",
        });
      }
      return runs;
    }
    case "failed":
      return [{ text: "Failed", ink: "error" }];
    case "interrupted":
      return [{ text: "Interrupted", ink: "warning" }];
    default:
      return null;
  }
}

/** Returns the usage segment one run reads as, when its projection carries one. */
function usageRun(facts: StatusFacts): StatusRun | null {
  if (facts.usage === null) {
    return null;
  }
  return facts.usage === "not_reported"
    ? { text: "ctx not reported", ink: "inkMuted" }
    : { text: `ctx in ${facts.usage.input} out ${facts.usage.output}`, ink: "inkMuted" };
}

/** Returns every styled run of the status row, in reading order. */
export function statusRuns(facts: StatusFacts): StatusRun[] {
  const runs: StatusRun[] = [connectionRun(facts.connection), { text: SEPARATOR, ink: "inkFaint" }];
  runs.push(
    facts.sessionId === null
      ? { text: "no session", ink: "inkFaint" }
      : { text: `session ${shortIdentifier(facts.sessionId)}`, ink: "inkMuted" },
  );
  if (facts.mode !== null) {
    runs.push({ text: SEPARATOR, ink: "inkFaint" });
    runs.push({ text: facts.mode, ink: "inkMuted" });
  }
  const phrase = runPhrase(facts);
  if (phrase !== null) {
    runs.push({ text: SEPARATOR, ink: "inkFaint" }, ...phrase);
  }
  const usage = usageRun(facts);
  if (usage !== null) {
    runs.push({ text: SEPARATOR, ink: "inkFaint" }, usage);
  }
  return runs;
}

/** Returns the text the detail line prints, and the role it paints. */
export function detailRuns(facts: {
  readonly error: string | null;
  readonly notice: string | null;
}): StatusRun[] {
  if (facts.error !== null) {
    return [{ text: facts.error, ink: "error" }];
  }
  if (facts.notice !== null) {
    return [{ text: facts.notice, ink: "inkMuted" }];
  }
  return [{ text: DEFAULT_HINT, ink: "inkFaint" }];
}

/** Returns the runs with the palette's own ink resolved. */
function inked(runs: readonly StatusRun[], palette: Palette) {
  return runs.map((run) => ({ text: run.text, ink: palette[run.ink] }));
}

/** Renders the status row from the facts it reads. */
export function StatusRow({ facts, palette }: { facts: StatusFacts; palette: Palette }) {
  return <Runs runs={inked(statusRuns(facts), palette)} />;
}

/** Renders the detail line from the facts it reads. */
export function DetailLine({
  error,
  notice,
  palette,
}: {
  error: string | null;
  notice: string | null;
  palette: Palette;
}) {
  return <Runs runs={inked(detailRuns({ error, notice }), palette)} />;
}

/**
 * Renders the status row from the store.
 *
 * The row subscribes to the facts it prints, so a streamed phase, a measured
 * tenth, and a committed usage each repaint this one row.
 */
export function StatusLine({ store, palette }: { store: AppStore; palette: Palette }) {
  const connection = useStore(store, selectConnection);
  const sessionId = useStore(store, selectSessionId);
  const mode = useStore(store, selectSessionMode);
  const phase = useStore(store, selectRunPhase);
  const runStatus = useStore(store, selectRunStatus);
  const elapsed = useStore(store, selectElapsed);
  const usage = useStore(store, selectUsage);
  const startedAt = useStore(store, selectStartedAt);
  const finishedAt = useStore(store, selectFinishedAt);
  const turnStartedAt = useStore(store, selectTurnStartedAt);
  useRunClock(store, turnStartedAt, phase !== null);
  return (
    <StatusRow
      facts={{
        connection,
        sessionId,
        mode,
        phase,
        runStatus,
        elapsed,
        usage,
        startedAt,
        finishedAt,
      }}
      palette={palette}
    />
  );
}

/** Renders the detail line from the store. */
export function Detail({ store, palette }: { store: AppStore; palette: Palette }) {
  const error = useStore(store, selectError);
  const notice = useStore(store, selectNotice);
  return <DetailLine error={error} notice={notice} palette={palette} />;
}
