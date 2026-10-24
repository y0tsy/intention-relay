//! The row a finished turn closes with: its outcome, its time, and its tokens.
//!
//! The line is the transcript's own account of a run, printed once the run
//! reaches a terminal status, so a reader scrolling back sees what each turn did
//! and cost without the footer's live status row. Every fact is the committed run
//! projection's: the status, the whole seconds between its start and its finish,
//! and the tokens the provider reported.

import type { RunProjectionDto } from "../../bindings";
import { markerText, Stack } from "../../kit";
import type { Palette } from "../../theme/palette";
import { RUN_GLYPH } from "../glyphs";
import { tokenCount } from "../status-phrases";

/** Returns the words a terminal status prints. */
function outcome(status: RunProjectionDto["status"]): string {
  switch (status) {
    case "failed":
      return "run failed";
    case "interrupted":
      return "run interrupted";
    default:
      return "run complete";
  }
}

/** Returns the row a finished run closes with. */
export function runMetaText(run: RunProjectionDto): string {
  const parts = [outcome(run.status)];
  const finished = run.finished_at;
  if (finished !== null && finished !== undefined) {
    parts.push(`${finished > run.started_at ? finished - run.started_at : 0}s`);
  }
  const usage = run.usage;
  if (usage !== null && usage !== undefined && usage.state === "reported") {
    parts.push(`${tokenCount(usage.input_tokens)} in / ${tokenCount(usage.output_tokens)} out`);
  }
  return parts.join(" · ");
}

/** Renders the row a finished run closes with. */
export function RunMeta({ run, palette }: { run: RunProjectionDto; palette: Palette }) {
  return (
    <Stack>
      <text fg={palette.inkFaint}>{markerText(RUN_GLYPH, runMetaText(run))}</text>
    </Stack>
  );
}
