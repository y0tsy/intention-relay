//! The status row and the detail line: the phrases painted, and the store read.
//!
//! The words themselves live in `status-phrases.ts`; this module resolves their
//! palette roles, subscribes to the facts they print, and paints the runs.

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
import type { AppStore } from "../app/store";
import { Runs, useContentColumns } from "../kit";
import type { Palette } from "../theme/palette";
import { useRunClock } from "./run-clock";
import { detailRuns, type StatusFacts, type StatusRun, statusRuns } from "./status-phrases";
import { useStore } from "./use-store";

export { DEFAULT_HINT, detailRuns, shortIdentifier, statusRuns } from "./status-phrases";
export type { StatusFacts, StatusRun } from "./status-phrases";

/** Returns the runs with the palette's own ink resolved. */
function inked(runs: readonly StatusRun[], palette: Palette) {
  return runs.map((run) => ({ text: run.text, ink: palette[run.ink] }));
}

/** Renders the status row from the facts it reads. */
export function StatusRow({
  facts,
  palette,
  columns,
}: {
  facts: StatusFacts;
  palette: Palette;
  /** The row's own columns; the inherited content width when omitted. */
  columns?: number;
}) {
  return <Runs runs={inked(statusRuns(facts, columns), palette)} />;
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
  const columns = useContentColumns();
  useRunClock(store, turnStartedAt, phase !== null);
  return (
    <StatusRow
      columns={columns}
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
