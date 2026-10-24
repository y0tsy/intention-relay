//! A durable tool exchange: one block, drawn at the call's position.
//!
//! A call and the result that answers it are two committed rows sharing a call
//! identity, and the transcript draws them once: the call's row owns the block,
//! the result's row contributes no block of its own. Everything the block shows
//! comes from the rows' typed projections - the tool identity, the status, and
//! the content the tool produced - so nothing here re-parses a document or
//! invents an argument.
//!
//! An `execute` result is never printed in any form: it is the command's raw
//! output, and showing it would dump the command's bytes into the transcript.
//! The guard is the first check in the preview, so no code path can reach it;
//! the process status the block prints instead is the typed evidence of what the
//! command did.

import type { MessageProjectionDto } from "../../bindings";
import { Lines, markerText, Pane } from "../../kit";
import type { Palette } from "../../theme/palette";
import { TOOL_GLYPH, TOOL_LIST_GLYPH } from "../glyphs";

/** The display rows a tool block previews before it bounds its content. */
export const TOOL_PREVIEW_ROWS = 12;

/** The note a call without a committed result row shows. */
const NOTE_NO_RESULT = "no tool_result row is committed";

/** The note a result without a committed call row shows. */
const NOTE_NO_CALL = "the call row for this result is not committed, so its arguments are unknown";

/** The note a failed result shows. */
const NOTE_FAILED = "the tool reported a failure";

/** The note a partial result shows. */
const NOTE_PARTIAL = "the tool stopped before a final result; the output above is partial";

/** The tool whose output is never printed. */
const EXECUTE = "execute";

/** Returns the notes a result's status asks for. */
function statusNotes(status: string | null): string[] {
  switch (status) {
    case "failed":
      return [NOTE_FAILED];
    case "partial":
      return [NOTE_PARTIAL];
    default:
      return [];
  }
}

/** Renders one tool exchange at the columns its parent gives it. */
export function ToolBlock({
  call,
  result,
  palette,
}: {
  call: MessageProjectionDto | null;
  result: MessageProjectionDto | null;
  palette: Palette;
}) {
  const tool = call?.tool ?? result?.tool ?? null;
  const identity = tool === null ? "tool" : tool.value.tool_id;
  const status = result?.tool?.row === "result" ? result.tool.value.status : null;
  const notes = [
    ...(call === null ? [NOTE_NO_CALL] : []),
    ...(call !== null && result === null ? [NOTE_NO_RESULT] : []),
    ...statusNotes(status),
  ];
  // A finished execute carries the command's output; the block says what the
  // process did and never repeats a byte of it.
  const printed = result?.text ?? "";
  const previewable = result !== null && identity !== EXECUTE && printed.trim().length > 0;
  return (
    <Pane
      title={markerText(TOOL_GLYPH, identity)}
      titleColor={palette.toolLabel}
      borderColor={palette.toolBorder}
      background={palette.toolSurface}
      paddingLeft={1}
      paddingRight={1}
    >
      {previewable ? (
        <Lines
          text={printed}
          rows={TOOL_PREVIEW_ROWS}
          ink={palette.toolPreview}
          rail={TOOL_LIST_GLYPH}
          overflow={(hidden) => (
            <text wrapMode="none" fg={palette.inkFaint}>{`… ${hidden} more rows`}</text>
          )}
        />
      ) : null}
      {notes.map((note) => (
        <text key={note} wrapMode="word" fg={palette.inkFaint}>
          {note}
        </text>
      ))}
    </Pane>
  );
}
