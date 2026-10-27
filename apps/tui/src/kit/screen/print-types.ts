//! The values one print carries: its block identity, its palette, and its job.

import type { Lead } from "./print-plan";

/** One block the printer can commit: its stable identity. */
export interface PrintedBlock {
  readonly key: string;
}

/** The palette roles the printed page paints. */
export interface PrintPalette {
  readonly canvas: string;
  readonly border: string;
}

/** The step in flight, as the page paints it. */
export interface StepPrint {
  /** The reasoning the step has streamed. */
  readonly reasoning: string;
  /** The answer the step has streamed. */
  readonly text: string;
}

/** One print on its way to the terminal: what it carries, and how far it got. */
export interface PrintJob<Block extends PrintedBlock> {
  /** The session the blocks belong to. */
  readonly session: string | null;
  /** The blocks this print carries, frozen when it was measured. */
  readonly blocks: readonly Block[];
  /** The step in flight this print paints, frozen when it was measured. */
  readonly step: StepPrint | null;
  /** The rows the print leads with. */
  readonly lead: Lead;
  /** The rows the surface measured, lead rows included; 0 before the measure. */
  readonly rows: number;
  /** The row the next chunk starts at. */
  readonly next: number;
  /** The rows the footer has given back to this print, which are ready to write. */
  readonly given: number;
  /** The last row the print may write: all of it, or the part the stream sealed. */
  readonly sealed: number;
  /** Whether the step in flight still grows: a closed step seals its edge. */
  readonly open: boolean;
}
