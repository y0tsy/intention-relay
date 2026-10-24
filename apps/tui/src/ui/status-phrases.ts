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
