//! The prompt history a session keeps.
//!
//! History is a plain list with a cursor into it: the cursor starts at the end
//! (nothing recalled), moves back through earlier prompts, and returns to the
//! live draft when it moves past the newest one. One entry is kept per
//! submission, and a repeat of the newest entry is not stored twice.

/** The prompts one front end has submitted, newest last. */
export class PromptHistory {
  readonly #entries: string[] = [];
  #cursor: number | null = null;

  /** Returns every submitted prompt, oldest first. */
  get entries(): readonly string[] {
    return this.#entries;
  }

  /** Records one submitted prompt. */
  push(prompt: string): void {
    const trimmed = prompt.trim();
    if (trimmed.length === 0) {
      return;
    }
    if (this.#entries.at(-1) === trimmed) {
      this.#cursor = null;
      return;
    }
    this.#entries.push(trimmed);
    this.#cursor = null;
  }

  /**
   * Moves `delta` entries back through history and returns the prompt to show.
   *
   * `null` means no prompt to show: the walk has returned to the live draft, or
   * there is nothing recorded yet. A caller that keeps its draft keeps it as the
   * fallback, so a cancelled draft is never lost to an accidental arrow key.
   */
  navigate(delta: number): string | null {
    const last = this.#entries.length - 1;
    if (last < 0) {
      return null;
    }
    if (this.#cursor === null) {
      // The walk starts at the live draft: moving back recalls the newest
      // prompt, and moving forward has nowhere left to go.
      if (delta >= 0) {
        return null;
      }
      this.#cursor = last;
      return this.#entries[last] ?? null;
    }
    const next = this.#cursor + delta;
    if (next > last) {
      this.#cursor = null;
      return null;
    }
    this.#cursor = Math.max(0, next);
    return this.#entries[this.#cursor] ?? null;
  }

  /** Returns whether the walk is at the live draft rather than an entry. */
  get atLive(): boolean {
    return this.#cursor === null;
  }
}
