//! The buffer the reader types into: its window around the caret.
//!
//! A buffer taller than the rows the input keeps shows the window of lines that
//! carries the caret's line; a line longer than the panel shows the window of
//! columns that carries the caret's cell. The draft itself is never capped: only
//! the window moves.

import { displayWidth } from "../text";
import { useContentColumns, useSlotRows } from "./context";

/** The rows the input's chrome takes around its buffer lines: the frame, the badge, and the status. */
export const INPUT_CHROME_ROWS = 4;

/** The glyph rendered at the input cursor position. */
export const INPUT_CURSOR = "█";

/** The prompt the input buffer's first line carries before its text. */
export const INPUT_PROMPT = "> ";

/** The indent every continuation row of the buffer carries before its text. */
export const INPUT_CONTINUATION = "  ";

/** Returns the rows the input keeps for one buffer: its chrome and its lines. */
export function inputSlotRows(text: string): number {
  return INPUT_CHROME_ROWS + Math.max(1, text.split("\n").length);
}

/** One buffer line split around the cursor, in display columns. */
export interface CursorWindow {
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
export function cursorWindow(line: string, cursor: number, columns: number): CursorWindow {
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
export function locateCaret(text: string, caret: number): { line: number; column: number } {
  const at = Math.max(0, Math.min(caret, text.length));
  const before = text.slice(0, at);
  const line = before.split("\n").length - 1;
  const column = before.length - (before.lastIndexOf("\n") + 1);
  return { line, column };
}

/** Returns the first line of the window that keeps the cursor's line visible. */
export function windowStart(cursorLine: number, visible: number): number {
  return cursorLine < visible ? 0 : cursorLine - visible + 1;
}

/** The palette roles one buffer row paints. */
export interface TextFieldPalette {
  readonly accent: string;
  readonly inputBg: string;
  readonly ink: string;
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
  palette: TextFieldPalette;
}) {
  const prefix = first ? INPUT_PROMPT : INPUT_CONTINUATION;
  const body =
    cursor === null ? { before: line, after: "" } : cursorWindow(line, cursor, columns - 2);
  const text = cursor === null ? body.before : `${body.before}${INPUT_CURSOR}${body.after}`;
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

/**
 * Renders the window of the buffer's lines that carries the caret.
 *
 * The slot comes from the footer's plan; the columns come from the pane the
 * input sits in. A caller that already knows both hands them in, otherwise the
 * brick reads the contexts the screen provides.
 */
export function TextField({
  text,
  caret,
  rows,
  columns,
  palette,
}: {
  text: string;
  caret: number;
  rows?: number;
  columns?: number;
  palette: TextFieldPalette;
}) {
  const inheritedSlot = useSlotRows(inputSlotRows(text));
  const inheritedColumns = useContentColumns();
  const slot = rows ?? inheritedSlot;
  const content = columns ?? inheritedColumns;
  const visible = Math.max(1, slot - INPUT_CHROME_ROWS);
  const caretAt = locateCaret(text, caret);
  const start = windowStart(caretAt.line, visible);
  const lines = text.split("\n");
  return (
    <box flexDirection="column" height={visible}>
      {lines.slice(start, start + visible).map((line, offset) => {
        const index = start + offset;
        return (
          <BufferRow
            key={index}
            first={index === 0}
            line={line}
            cursor={index === caretAt.line ? caretAt.column : null}
            columns={content}
            palette={palette}
          />
        );
      })}
    </box>
  );
}
