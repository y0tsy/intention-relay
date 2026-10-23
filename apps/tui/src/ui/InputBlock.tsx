//! The input block: the badge header, one row per buffer line, and the status
//! row under them.
//!
//! The block grows with its buffer, one row per line beside its own chrome, up to
//! the share of the panel's body the panel grants it; a buffer taller than the
//! rows it was given shows the window of lines that carries the cursor's line,
//! exactly as a line longer than the block shows the window of columns that
//! carries the cursor's cell. Neither a line's count nor a line's length is
//! capped in the draft itself.
//!
//! The badge header names the session's real run mode and the effective provider
//! model its projection carries. Both are projection facts, so a session no
//! snapshot has named yet shows no badge rather than a fabricated one.

import { selectDraft, selectSessionMode, selectSessionModel } from "../app/select";
import type { AppStore } from "../app/store";
import type { Palette } from "../theme/palette";
import { CONTINUATION_INDENT, CURSOR, PROMPT } from "./glyphs";
import { displayWidth } from "./measure";
import { StatusLine } from "./StatusRow";
import { useStore } from "./use-store";

/** The rows the block's frame takes. */
const BLOCK_FRAME = 2;

/** The rows the badge header occupies. */
const BADGE_ROWS = 1;

/** The rows the run status under the buffer occupies. */
const STATUS_ROWS = 1;

/** The rows the block's chrome takes around its buffer lines. */
export const BLOCK_CHROME_ROWS = BLOCK_FRAME + BADGE_ROWS + STATUS_ROWS;

/** One buffer line split around the cursor, in display columns. */
interface CursorWindow {
  /** The text before the cursor that fits. */
  readonly before: string;
  /** The text after the cursor that fits. */
  readonly after: string;
}

/**
 * Returns the characters of one line on either side of the cursor.
 *
 * The window ends at the cursor: when the text before it does not fit, the line
 * scrolls so the cursor is the window's last column.
 */
function cursorWindow(line: string, cursor: number, columns: number): CursorWindow {
  const at = Math.max(0, Math.min(cursor, line.length));
  const head = line.slice(0, at);
  const tail = line.slice(at);
  if (displayWidth(head) + 1 + displayWidth(tail) <= columns) {
    return { before: head, after: tail };
  }
  let start = at;
  while (start > 0 && displayWidth(line.slice(start - 1, at)) + 1 <= columns) {
    start -= 1;
  }
  return { before: line.slice(start, at), after: "" };
}

/** Returns the index of the line the caret sits on, and its column. */
function locate(text: string, caret: number): { line: number; column: number } {
  const at = Math.max(0, Math.min(caret, text.length));
  const before = text.slice(0, at);
  const line = before.split("\n").length - 1;
  const column = before.length - (before.lastIndexOf("\n") + 1);
  return { line, column };
}

/** Returns the first line of the window that keeps the cursor's line visible. */
function windowStart(cursorLine: number, visible: number): number {
  return cursorLine < visible ? 0 : cursorLine - visible + 1;
}

/** Renders one badge chip on its own wash. */
function Badge({ label, palette }: { label: string; palette: Palette }) {
  return (
    <text bg={palette.badgeBg} fg={palette.badgeInk}>
      {` ${label} `}
    </text>
  );
}

/** Renders the badge header: the mode and the model, each on its own chip. */
function Badges({
  mode,
  model,
  palette,
}: {
  mode: string | null;
  model: string | null;
  palette: Palette;
}) {
  return (
    <box flexDirection="row" height={BADGE_ROWS}>
      {mode === null ? null : <Badge label={mode} palette={palette} />}
      {mode !== null && model !== null ? <text> </text> : null}
      {model === null ? null : <Badge label={model} palette={palette} />}
    </box>
  );
}

/** Renders one buffer row: its prompt or indent, its text, and the cursor. */
function BufferRow({
  first,
  line,
  cursor,
  columns,
  palette,
}: {
  first: boolean;
  line: string;
  cursor: number | null;
  columns: number;
  palette: Palette;
}) {
  const prefix = first ? PROMPT : CONTINUATION_INDENT;
  const body =
    cursor === null ? { before: line, after: "" } : cursorWindow(line, cursor, columns - 2);
  const text = cursor === null ? body.before : `${body.before}${CURSOR}${body.after}`;
  return (
    <box flexDirection="row" height={1} backgroundColor={palette.inputBg}>
      <text fg={first ? palette.accent : palette.inputBg} attributes={first ? 1 : undefined}>
        {prefix}
      </text>
      <text fg={palette.ink} wrapMode="none">
        {text}
      </text>
    </box>
  );
}

/** Renders the input block: its frame, the badges, the buffer, and the status. */
export function InputBlock({
  store,
  palette,
  rows,
  columns,
}: {
  store: AppStore;
  palette: Palette;
  rows: number;
  columns: number;
}) {
  const draft = useStore(store, selectDraft);
  const mode = useStore(store, selectSessionMode);
  const model = useStore(store, selectSessionModel);
  const lines = draft.text.split("\n");
  const caret = locate(draft.text, draft.caret);
  const visible = Math.max(1, rows - BLOCK_CHROME_ROWS);
  const start = windowStart(caret.line, visible);
  return (
    <box
      height={rows}
      flexDirection="column"
      borderStyle="rounded"
      border
      borderColor={palette.accent}
      backgroundColor={palette.panel}
      paddingLeft={1}
      paddingRight={1}
    >
      <Badges mode={mode} model={model} palette={palette} />
      {lines.slice(start, start + visible).map((line, offset) => {
        const index = start + offset;
        return (
          <BufferRow
            key={index}
            first={index === 0}
            line={line}
            cursor={index === caret.line ? caret.column : null}
            columns={columns}
            palette={palette}
          />
        );
      })}
      <box height={STATUS_ROWS}>
        <StatusLine store={store} palette={palette} />
      </box>
    </box>
  );
}
