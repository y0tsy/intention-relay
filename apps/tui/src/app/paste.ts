//! The pastes a terminal delivered, and the one label a large one leaves.
//!
//! A paste is one edit, never the keystrokes its characters look like: the
//! terminal delivers the text, and a text that carries a line break - or more
//! characters than the inline limit - is kept outside the buffer behind one
//! labelled line, because a line the user cannot read at a glance is not a line
//! they can edit. The label names the paste's own beginning and how many lines
//! it carries, and the submission path expands it back into the whole text, so
//! the session receives the paste and not its summary.

/** The characters a paste may carry and still be inserted as ordinary text. */
export const PASTE_INLINE_CHARS = 400;

/** The lines a paste may carry and still be inserted as ordinary text. */
export const PASTE_INLINE_LINES = 8;

/** The characters of a paste label's head: the text's own beginning. */
export const PASTE_HEAD_CHARS = 50;

/** The prefix every paste label carries. */
export const PASTE_LABEL_PREFIX = "[pasted ";

/** One pasted text the input line stands for with one labelled line. */
export interface PastedText {
  /** The labelled line that stands for the text inside the buffer. */
  readonly label: string;
  /** The pasted text, exactly as the terminal delivered it. */
  readonly text: string;
}

/** One label's span inside the input line. */
export interface LabelSpan {
  /** The index of the label's first character. */
  readonly start: number;
  /** The index after the label's last character. */
  readonly end: number;
  /** The index of the paste the label stands for. */
  readonly index: number;
}

/**
 * Returns one paste with the line breaks a terminal may have carried read as
 * newlines.
 *
 * A terminal hands a paste over with the line endings of whatever it copied
 * from, so the buffer keeps one spelling of a line break and the session
 * receives the text the user meant.
 */
export function normalisePaste(text: string): string {
  return text.replaceAll("\r\n", "\n").replaceAll("\r", "\n");
}

/** Returns whether one paste is too large for the input line to show. */
export function pasteIsLarge(text: string): boolean {
  return [...text].length > PASTE_INLINE_CHARS || text.split("\n").length > PASTE_INLINE_LINES;
}

/** Returns the label one stored paste is shown as. */
export function pasteLabel(text: string): string {
  const lines = Math.max(1, text.split("\n").length);
  const unit = lines === 1 ? "line" : "lines";
  return `${PASTE_LABEL_PREFIX}${pasteHead(text)}... ${lines} ${unit}]`;
}

/**
 * Returns the first characters of one paste, read as one line of prose.
 *
 * The characters an input line cannot show are dropped and the line breaks and
 * tabs inside the head read as the spaces between words, so a paste that starts
 * with an indented line still shows its own words in the label.
 */
export function pasteHead(text: string): string {
  return [...text]
    .filter((character) => !isUnshowableControl(character))
    .slice(0, PASTE_HEAD_CHARS)
    .map((character) => (character === "\n" || character === "\t" ? " " : character))
    .join("")
    .trim();
}

/** Returns whether one character is one an input line cannot show. */
function isUnshowableControl(character: string): boolean {
  const code = character.codePointAt(0) ?? 0;
  return code === 0x7f || (code < 0x20 && character !== "\n" && character !== "\t");
}

/**
 * Returns the span of every label the input line carries, in the order the
 * labels appear.
 *
 * A label is one unit for the caret and the delete keys, and the buffer is the
 * only place a label's position is recorded, so the spans are read back out of
 * the text: a label the user edited into something else is no longer found and
 * is ordinary text from then on. The search resumes after each match, so a
 * paste inserted twice is found twice, in order.
 */
export function pasteSpans(text: string, pastes: readonly PastedText[]): readonly LabelSpan[] {
  const spans: LabelSpan[] = [];
  let from = 0;
  pastes.forEach((paste, index) => {
    const start = text.indexOf(paste.label, from);
    if (start < 0) {
      return;
    }
    spans.push({ start, end: start + paste.label.length, index });
    from = start + paste.label.length;
  });
  return spans;
}

/**
 * Returns `text` with every label replaced by the text it stands for.
 *
 * A label the buffer no longer carries is dropped: what the user edited is the
 * line they see, so a label that was typed over contributes nothing.
 */
export function expandPastes(text: string, pastes: readonly PastedText[]): string {
  const spans = pasteSpans(text, pastes);
  let expanded = "";
  let at = 0;
  for (const span of spans) {
    expanded += text.slice(at, span.start) + (pastes[span.index]?.text ?? "");
    at = span.end;
  }
  return expanded + text.slice(at);
}
