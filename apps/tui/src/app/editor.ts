//! Editing one prompt draft.
//!
//! The draft is text plus a caret index, and every edit is a pure function of
//! the two, so the composer's behaviour is decided here and tested without a
//! terminal.

/** One prompt draft: the text and the caret's index into it. */
export interface Draft {
  /** The draft's text, exactly as the user composed it. */
  readonly text: string;
  /** The caret's index: the count of code units before it. */
  readonly caret: number;
}

/** The empty draft. */
export const EMPTY_DRAFT: Draft = { text: "", caret: 0 };

/** One caret edit a key names, as the state machine receives it. */
export type DraftEdit = "backspace" | "delete" | "left" | "right";

/** Inserts text at the caret and leaves the caret after it. */
export function insertText(draft: Draft, text: string): Draft {
  const before = draft.text.slice(0, draft.caret);
  const after = draft.text.slice(draft.caret);
  return { text: `${before}${text}${after}`, caret: draft.caret + text.length };
}

/** Inserts one line break at the caret. */
export function insertNewline(draft: Draft): Draft {
  return insertText(draft, "\n");
}

/** Removes the character before the caret. */
export function backspace(draft: Draft): Draft {
  if (draft.caret === 0) {
    return draft;
  }
  const before = draft.text.slice(0, draft.caret - 1);
  const after = draft.text.slice(draft.caret);
  return { text: `${before}${after}`, caret: draft.caret - 1 };
}

/** Removes the character after the caret. */
export function deleteForward(draft: Draft): Draft {
  if (draft.caret >= draft.text.length) {
    return draft;
  }
  const before = draft.text.slice(0, draft.caret);
  const after = draft.text.slice(draft.caret + 1);
  return { text: `${before}${after}`, caret: draft.caret };
}

/** Moves the caret by `delta` characters, stopping at either end. */
export function moveCaret(draft: Draft, delta: number): Draft {
  const caret = Math.min(draft.text.length, Math.max(0, draft.caret + delta));
  return { text: draft.text, caret };
}

/** Moves the caret to the start of the draft. */
export function caretToStart(draft: Draft): Draft {
  return { text: draft.text, caret: 0 };
}

/** Moves the caret to the end of the draft. */
export function caretToEnd(draft: Draft): Draft {
  return { text: draft.text, caret: draft.text.length };
}

/** Applies one of the four caret edits the composer's keys name. */
export function editDraft(draft: Draft, edit: DraftEdit): Draft {
  switch (edit) {
    case "backspace":
      return backspace(draft);
    case "delete":
      return deleteForward(draft);
    case "left":
      return moveCaret(draft, -1);
    case "right":
      return moveCaret(draft, 1);
  }
}

/** Replaces the whole draft with one line, as history navigation does. */
export function replaceDraft(text: string): Draft {
  return { text, caret: text.length };
}

/**
 * Returns the text of the character the caret sits before.
 *
 * A surrogate pair counts as one character, so backspace never splits an emoji
 * into two replacement halves.
 */
export function characterAt(text: string, index: number): number {
  const code = text.codePointAt(index);
  if (code === undefined) {
    return 1;
  }
  return code > 0xffff ? 2 : 1;
}

/** Returns the text the composer shows, with the caret marked. */
export function renderDraft(draft: Draft, marker = "▏"): string {
  return `${draft.text.slice(0, draft.caret)}${marker}${draft.text.slice(draft.caret)}`;
}
