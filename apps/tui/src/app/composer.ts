//! The composer: the draft line, its history, its pastes, and its command band.
//!
//! The composer owns every edit a key makes before it becomes a turn. A paste
//! larger than the line can show leaves one labelled line behind instead of
//! filling the draft with text the reader cannot see; a recalled prompt comes
//! back whole; and the band follows the word the caret sits in without the
//! caller deciding what that word means.

import type { Arm } from "./arm";
import {
  type Draft,
  type DraftEdit,
  EMPTY_DRAFT,
  editDraft,
  insertNewline,
  insertText,
  moveCaretLine,
  replaceDraft,
} from "./editor";
import { PromptHistory } from "./history";
import { type CommandMenu, commandMenu, menuCompletion, menuKey } from "./menu";
import {
  expandPastes,
  normalisePaste,
  type PastedText,
  pasteIsLarge,
  pasteLabel,
  pasteSpans,
} from "./paste";

/** Owns the input line and everything a keystroke does to it. */
export class Composer {
  /** The prompt line the composer shows, and where its caret sits. */
  draft: Draft = EMPTY_DRAFT;
  /** The row of the command band the reader is on. */
  menuSelection = 0;
  /** What the last layered key press armed for its next consecutive press. */
  armed: Arm | null = null;
  /** The prompts this front end has submitted, for the arrow keys to walk. */
  readonly #history = new PromptHistory();
  /** The pastes the input line stands for with one labelled line each. */
  #pastes: readonly PastedText[] = [];
  /** The word the band's cursor belongs to, which a new word resets. */
  #menuWord: string | null = null;

  /** Returns the rows the band shows for the word the caret sits in, if any. */
  get menu(): CommandMenu | null {
    return commandMenu(this.draft);
  }

  /**
   * Returns the row of the command band the reader is on.
   *
   * The cursor is the state's, and it is clamped to the rows the current word
   * ranks: a keystroke that narrows the list leaves the cursor on a row that
   * exists, and a band with no rows cursors nothing. The walk stops at the first
   * and last row instead of wrapping, which is the same bounded walk the
   * session browser's cursor makes.
   */
  get menuHighlight(): number {
    const rows = this.menu?.rows.length ?? 0;
    if (rows === 0) {
      return 0;
    }
    return Math.min(rows - 1, Math.max(0, this.menuSelection));
  }

  /** Returns whether the line carries text a turn could submit. */
  get hasText(): boolean {
    return this.draft.text.trim() !== "";
  }

  /** Returns the line with every paste label expanded to the text it stands for. */
  expanded(): string {
    return expandPastes(this.draft.text, this.#pastes);
  }

  /** Resets the band's cursor whenever the word it ranks is another word. */
  refreshMenu(): void {
    const menu = this.menu;
    const word = menu === null ? null : menuKey(menu);
    if (word !== this.#menuWord) {
      this.#menuWord = word;
      this.menuSelection = 0;
    }
  }

  /** Types one printable character into the line. */
  type(text: string): void {
    this.draft = insertText(this.draft, text);
    this.refreshMenu();
  }

  /** Puts a line break into the line. */
  newline(): void {
    this.draft = insertNewline(this.draft);
    this.refreshMenu();
  }

  /** Moves the band's cursor one row. */
  moveMenu(delta: number): void {
    this.menuSelection += delta;
  }

  /**
   * Commits the band's highlighted row into the input line.
   *
   * The word the band was opened for is replaced by the row's completion, and
   * the band then follows the same open rule: a command that declares an
   * argument reopens it on the argument word, and a complete command or value
   * closes it.
   */
  acceptMenu(): void {
    const menu = this.menu;
    if (menu === null) {
      return;
    }
    const completion = menuCompletion(menu, this.menuHighlight);
    if (completion === null) {
      return;
    }
    const text =
      this.draft.text.slice(0, completion.start) +
      completion.text +
      this.draft.text.slice(completion.end);
    this.draft = { text, caret: completion.start + completion.text.length };
    this.refreshMenu();
  }

  /**
   * Applies one caret edit, with a paste label read as one unit.
   *
   * Removing one character of a label would leave a label standing for nothing,
   * so the whole label goes at once, exactly as it was inserted, and the caret
   * steps over one instead of into it.
   */
  edit(edit: DraftEdit): void {
    const spans = pasteSpans(this.draft.text, this.#pastes);
    const caret = this.draft.caret;
    const removal =
      edit === "backspace"
        ? spans.find((span) => span.start < caret && caret <= span.end)
        : edit === "delete"
          ? spans.find((span) => span.start === caret)
          : undefined;
    if (removal !== undefined) {
      this.draft = {
        text: this.draft.text.slice(0, removal.start) + this.draft.text.slice(removal.end),
        caret: removal.start,
      };
      this.#pastes = this.#pastes.filter((_, index) => index !== removal.index);
      this.refreshMenu();
      return;
    }
    const step = spans.find((span) =>
      edit === "left" ? span.end === caret : edit === "right" ? span.start === caret : false,
    );
    if (step !== undefined) {
      this.draft = { text: this.draft.text, caret: edit === "left" ? step.start : step.end };
      return;
    }
    this.draft = editDraft(this.draft, edit);
    this.refreshMenu();
  }

  /**
   * Applies one paste, as one edit.
   *
   * A paste the line can hold is inserted as the ordinary text it is; a paste
   * larger than the line can show leaves one labelled line behind and keeps its
   * text outside the buffer, because a line the user cannot read at a glance is
   * not a line they can edit.
   */
  paste(raw: string): void {
    const text = normalisePaste(raw);
    if (text === "") {
      return;
    }
    if (pasteIsLarge(text)) {
      const label = pasteLabel(text);
      this.draft = insertText(this.draft, label);
      this.#pastes = [...this.#pastes, { label, text }];
    } else {
      this.draft = insertText(this.draft, text);
    }
    this.refreshMenu();
  }

  /**
   * Recalls one earlier prompt into the input line.
   *
   * A walk that comes back to the live draft leaves what the user had typed
   * alone: the draft is the fallback, so an accidental arrow key never spends a
   * half-written prompt.
   */
  recall(delta: number): void {
    // A multi-line prompt is navigated the way its own rows are: the arrows
    // move the caret while the draft carries a line in that direction, and only
    // at the first and last line do they walk the history.
    const moved = moveCaretLine(this.draft, delta);
    if (moved !== null) {
      this.draft = moved;
      this.refreshMenu();
      return;
    }
    const recalled = this.#history.navigate(delta);
    if (recalled !== null) {
      this.draft = replaceDraft(recalled);
      this.#pastes = [];
      this.refreshMenu();
    }
  }

  /** Abandons the line into the recallable history and clears it. */
  clearIntoHistory(): void {
    const text = this.expanded();
    if (text.trim() !== "") {
      this.#history.push(text);
    }
    this.clear();
  }

  /** Remembers one submitted line for the arrow keys. */
  remember(text: string): void {
    this.#history.push(text);
  }

  /** Clears the line, its pastes, and the band's cursor. */
  clear(): void {
    this.draft = EMPTY_DRAFT;
    this.#pastes = [];
    this.refreshMenu();
  }
}
