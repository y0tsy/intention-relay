//! The chunks each reasoning block has revealed.
//!
//! A block that names no identity asks for the newest block there is: the live
//! segment while a step is reasoning, the last committed one after it, so the
//! gesture reveals what the reader is looking at.

import type { MessageProjectionDto } from "../bindings";

/** Owns the revealed depth of every reasoning block, by block identity. */
export class ReasoningExpansions {
  /** The chunks each reasoning block has revealed, keyed by block identity. */
  expansions: ReadonlyMap<string, number> = new Map();

  /** Returns the chunks one reasoning block has revealed. */
  of(id: string): number {
    return this.expansions.get(id) ?? 0;
  }

  /**
   * Reveals one more chunk of one reasoning block.
   *
   * A block that names no identity asks for the newest block there is, and an
   * identity with no block asks for nothing.
   */
  expand(
    id: string | null,
    transcript: readonly MessageProjectionDto[],
    provisionalReasoning: string,
  ): void {
    const key = id ?? this.#newestKey(transcript, provisionalReasoning);
    if (key === null) {
      return;
    }
    const next = new Map(this.expansions);
    next.set(key, (this.expansions.get(key) ?? 0) + 1);
    this.expansions = next;
  }

  /** Returns the identity of the newest block that has reasoning to reveal. */
  #newestKey(
    transcript: readonly MessageProjectionDto[],
    provisionalReasoning: string,
  ): string | null {
    if (provisionalReasoning.length > 0) {
      return "live";
    }
    for (let index = transcript.length - 1; index >= 0; index -= 1) {
      const message = transcript[index];
      if (
        message !== undefined &&
        message.reasoning !== null &&
        message.reasoning.trim().length > 0
      ) {
        return String(message.id);
      }
    }
    return null;
  }
}
