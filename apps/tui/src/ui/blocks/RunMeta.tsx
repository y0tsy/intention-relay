//! The row a finished turn closes with: its outcome, its time, and its tokens.
//!
//! The line is the transcript's own account of a run, printed once the run
//! reaches a terminal status, so a reader scrolling back sees what each turn did
//! and cost without the footer's live status row. Every fact is the committed run
//! projection's: the status, the whole seconds between its start and its finish,
//! and the tokens the provider reported.

import type { RunProjectionDto } from "../../bindings";
import { Runs, Stack } from "../../kit";
import type { Palette } from "../../theme/palette";
import { RUN_GLYPH, SEPARATOR } from "../glyphs";
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

/** Returns the ink a terminal status wears. */
function outcomeInk(status: RunProjectionDto["status"], palette: Palette): string {
  switch (status) {
    case "failed":
      return palette.error;
    case "interrupted":
      return palette.warning;
    default:
      return palette.success;
  }
}

/** Returns the facts that follow the outcome: the seconds and the tokens. */
function facts(run: RunProjectionDto): string {
  const parts: string[] = [];
  const finished = run.finished_at;
  if (finished !== null && finished !== undefined) {
    parts.push(`${finished > run.started_at ? finished - run.started_at : 0}s`);
  }
  const usage = run.usage;
  if (usage !== null && usage !== undefined && usage.state === "reported") {
    parts.push(`${tokenCount(usage.input_tokens)} in / ${tokenCount(usage.output_tokens)} out`);
  }
  return parts.join(SEPARATOR);
}

/** Returns the row a finished run closes with, as plain text. */
export function runMetaText(run: RunProjectionDto): string {
  const rest = facts(run);
  return rest.length === 0 ? outcome(run.status) : `${outcome(run.status)}${SEPARATOR}${rest}`;
}

/** Renders the row a finished run closes with. */
export function RunMeta({ run, palette }: { run: RunProjectionDto; palette: Palette }) {
  const rest = facts(run);
  return (
    <Stack>
      <Runs
        runs={[
          {
            text: `${RUN_GLYPH} ${outcome(run.status)}`,
            ink: outcomeInk(run.status, palette),
          },
          ...(rest.length === 0 ? [] : [{ text: `${SEPARATOR}${rest}`, ink: palette.inkFaint }]),
        ]}
      />
    </Stack>
  );
}
