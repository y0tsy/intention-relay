//! The formatting decisions the acceptance criteria turn on.
//!
//! Every rule that decides what a line shows lives here, away from the
//! components, so "a hint drops whole segments", "the detail line never speaks
//! the internal vocabulary", and "a narrow table drops a column" are decisions a
//! test can read.

import type { RunStatusDto, SessionSummaryDto } from "../bindings";

/** One segment of the hint band: a key and what it does. */
export interface HintSegment {
  /** The keys as a user presses them. */
  readonly keys: string;
  /** What pressing them does. */
  readonly action: string;
}

/** Every hint the band knows, in the order it offers them. */
export const HINT_SEGMENTS: readonly HintSegment[] = [
  { keys: "enter", action: "send" },
  { keys: "shift+enter", action: "new line" },
  { keys: "↑↓", action: "history" },
  { keys: "/", action: "commands" },
  { keys: "esc", action: "stop" },
  { keys: "ctrl+d", action: "quit" },
];

/** The separator between rendered hint segments, with its own spaces. */
const HINT_SEPARATOR = " · ";

/**
 * Returns the hint segments that fit in `width` columns.
 *
 * A segment is kept whole or dropped whole: a hint cut mid-word tells a reader
 * less than no hint at all, and the band is the one place in the app where space
 * is genuinely scarce. The first segment always fits, because the band is only
 * rendered where at least one key can be shown.
 */
export function fitHints(
  width: number,
  segments: readonly HintSegment[] = HINT_SEGMENTS,
): HintSegment[] {
  const fitted: HintSegment[] = [];
  let used = 0;
  for (const segment of segments) {
    const cost = segment.keys.length + 1 + segment.action.length;
    const separator = fitted.length === 0 ? 0 : HINT_SEPARATOR.length;
    if (fitted.length > 0 && used + separator + cost > width) {
      break;
    }
    // The first segment is kept even when it overflows: a band with no key at
    // all is a line of pure noise.
    fitted.push(segment);
    used += separator + cost;
  }
  return fitted;
}

/** Renders the hint segments that fit in `width` columns as one line. */
export function renderHints(
  width: number,
  segments: readonly HintSegment[] = HINT_SEGMENTS,
): string {
  return fitHints(width, segments)
    .map((segment) => `${segment.keys} ${segment.action}`)
    .join(HINT_SEPARATOR);
}

/**
 * Returns the one phrase that names a run's state.
 *
 * The phrase is the user's, not the protocol's: a reader is told what is
 * happening to their prompt, never which enum a run is in, so `run running`
 * never appears on a line a person reads.
 */
export function describeRun(status: RunStatusDto, hasProvisionalText: boolean): string {
  switch (status) {
    case "starting":
      return "starting";
    case "running":
      return hasProvisionalText ? "answering" : "thinking";
    case "completed":
      return "done";
    case "failed":
      return "failed";
    case "interrupted":
      return "stopped";
  }
}

/** Returns the elapsed time of a run, or `null` when it has not started. */
export function formatElapsed(startedAt: number | null, now: number): string | null {
  if (startedAt === null) {
    return null;
  }
  return formatDuration(Math.max(0, Math.floor(now - startedAt)));
}

/** Renders one span of seconds the way a status line reads it. */
export function formatDuration(seconds: number): string {
  if (seconds < 60) {
    return `${seconds}s`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
  }
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${String(minutes % 60).padStart(2, "0")}m`;
}

/** Renders how long ago one past timestamp was, at the coarsest useful unit. */
export function formatAgo(timestamp: number, now: number): string {
  const seconds = Math.max(0, Math.floor(now - timestamp));
  if (seconds < 60) {
    return "now";
  }
  if (seconds < 3_600) {
    return `${Math.floor(seconds / 60)}m`;
  }
  if (seconds < 86_400) {
    return `${Math.floor(seconds / 3_600)}h`;
  }
  return `${Math.floor(seconds / 86_400)}d`;
}

/** Returns the last eight characters of an identity, which is what distinguishes it. */
export function shortId(id: string): string {
  return id.length <= 8 ? id : id.slice(-8);
}

/** One column of the session table. */
export interface TableColumn {
  /** The column's header. */
  readonly header: string;
  /** The width the column needs, header included. */
  readonly width: number;
  /** How many columns from the right may be dropped before this one goes. */
  readonly priority: number;
}

/** Every column of the session table, widest priority first. */
export const SESSION_COLUMNS: readonly TableColumn[] = [
  { header: "session", width: 8, priority: 0 },
  { header: "updated", width: 6, priority: 1 },
  { header: "workspace", width: 18, priority: 2 },
  { header: "rows", width: 4, priority: 3 },
];

/**
 * Returns the columns that fit in `width` columns of terminal.
 *
 * A narrow table drops a column whole instead of squeezing every column into
 * nothing: a truncated identifier is not an identifier, and the row stays
 * readable as long as the columns it keeps are the ones that identify it.
 */
export function fitColumns(
  width: number,
  columns: readonly TableColumn[] = SESSION_COLUMNS,
): TableColumn[] {
  const fitted = [...columns];
  const gaps = (count: number): number => Math.max(0, count - 1) * 2;
  const measure = (): number =>
    fitted.reduce((total, column) => total + column.width, 0) + gaps(fitted.length);
  const droppable = [...fitted].sort((left, right) => right.priority - left.priority);
  for (const column of droppable) {
    if (measure() <= width || fitted.length === 1) {
      break;
    }
    const index = fitted.indexOf(column);
    fitted.splice(index, 1);
  }
  return fitted;
}

/** Returns one session's cells for the columns that fit. */
export function sessionCells(
  summary: SessionSummaryDto,
  columns: readonly TableColumn[],
  now: number,
): string[] {
  return columns.map((column) => {
    switch (column.header) {
      case "session":
        return shortId(summary.session_id);
      case "updated":
        return formatAgo(summary.updated_at, now);
      case "workspace":
        return basename(summary.workspace_root);
      case "rows":
        return String(summary.message_count);
      default:
        return "";
    }
  });
}

/** Returns the last path segment of a workspace root. */
export function basename(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const segments = trimmed.split(/[\\/]/);
  return segments.at(-1) ?? trimmed;
}

/**
 * Returns the text of one welcome prompt example that fits `width`.
 *
 * The examples are whole sentences: one that does not fit is dropped, never
 * shortened, so the welcome surface never shows a fragment of a suggestion.
 */
export function fitExamples(width: number, examples: readonly string[]): string[] {
  return examples.filter((example) => example.length <= width);
}
