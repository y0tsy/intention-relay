//! What the status row says: the facts it reads, and the runs they become.
//!
//! The phrases name what a reader acts on - the connection, the session, its
//! run mode, the live phase, and the committed usage - and the detail line
//! names the failure, the notice, or the idle hint. Neither renders anything,
//! so a test reads the words without a terminal.

import { committedSeconds, elapsedTenths, phaseWord, type RunPhase } from "../app/run";
import type { ConnectionStatus } from "../app/state";
import type { RunModeDto, RunStatusDto } from "../bindings";
import type { Palette } from "../theme/palette";
import { SEPARATOR } from "./glyphs";

/** The detail line this front end shows while no error or notice claims it. */
export const DEFAULT_HINT = "enter sends · /new · /sessions";

/** The detail line a wide window shows: every command the welcome offers. */
export const WIDE_HINT = "enter send · /new session · /sessions browse · /theme palette";

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
export function connectionStatus(connection: ConnectionStatus): StatusRun {
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

/** One fact the status row can keep or give up as the window narrows. */
interface StatusPiece {
  readonly runs: StatusRun[];
  /** Whether the piece may be dropped to fit; the connection never is. */
  readonly optional: boolean;
  /** The order in which optional pieces are dropped: lower goes first. */
  readonly rank: number;
}

/**
 * Returns the width of the pieces as one row, separators included.
 *
 * A status fact is renderer-safe - one cell per character - so a length is the
 * width: connection words, identifiers, modes, phases, and numbers, with only
 * one-cell `·` and `…` among them.
 */
function piecesWidth(pieces: readonly StatusPiece[]): number {
  const text = pieces.reduce(
    (total, piece) => total + piece.runs.reduce((runTotal, run) => runTotal + run.text.length, 0),
    0,
  );
  const separators = pieces.length > 1 ? pieces.length - 1 : 0;
  return text + separators * SEPARATOR.length;
}

/** Drops the least important facts until the row fits the columns it was given. */
function fitted(pieces: readonly StatusPiece[], columns: number): StatusPiece[] {
  let kept = [...pieces];
  if (piecesWidth(kept) <= columns) {
    return kept;
  }
  const droppable = kept.filter((piece) => piece.optional).sort((a, b) => a.rank - b.rank);
  for (const piece of droppable) {
    if (piecesWidth(kept) <= columns) {
      break;
    }
    kept = kept.filter((candidate) => candidate !== piece);
  }
  return kept;
}

/** Returns the pieces as one row of runs, separated where they meet. */
function joinPieces(pieces: readonly StatusPiece[]): StatusRun[] {
  return pieces.flatMap((piece, index) =>
    index === 0 ? piece.runs : [{ text: SEPARATOR, ink: "inkFaint" }, ...piece.runs],
  );
}

/**
 * Returns every styled run of the status row, in reading order.
 *
 * A narrow window gives up the usage, the mode, the session identity, and then
 * the run phrase, in that order: the connection and the live state are what a
 * reader acts on, so they are the last pieces to leave the row.
 */
export function statusRuns(facts: StatusFacts, columns = Number.POSITIVE_INFINITY): StatusRun[] {
  const pieces: StatusPiece[] = [
    { runs: [connectionStatus(facts.connection)], optional: false, rank: 0 },
    {
      runs:
        facts.sessionId === null
          ? [{ text: "no session", ink: "inkFaint" }]
          : [{ text: `session ${shortIdentifier(facts.sessionId)}`, ink: "inkMuted" }],
      optional: true,
      rank: 2,
    },
  ];
  if (facts.mode !== null) {
    pieces.push({ runs: [{ text: facts.mode, ink: "inkMuted" }], optional: true, rank: 1 });
  }
  const phrase = runPhrase(facts);
  if (phrase !== null) {
    pieces.push({ runs: phrase, optional: true, rank: 3 });
  }
  const usage = usageRun(facts);
  if (usage !== null) {
    pieces.push({ runs: [usage], optional: true, rank: 0 });
  }
  return joinPieces(fitted(pieces, columns));
}

/**
 * Returns the text the detail line prints, and the role it paints.
 *
 * A window wide enough for the full command line shows it; a narrower one keeps
 * the same sentence short enough to stay whole.
 */
export function detailRuns(
  facts: {
    readonly error: string | null;
    readonly notice: string | null;
  },
  columns = 0,
): StatusRun[] {
  if (facts.error !== null) {
    return [{ text: facts.error, ink: "error" }];
  }
  if (facts.notice !== null) {
    return [{ text: facts.notice, ink: "inkMuted" }];
  }
  return [{ text: columns >= 64 ? WIDE_HINT : DEFAULT_HINT, ink: "inkMuted" }];
}
