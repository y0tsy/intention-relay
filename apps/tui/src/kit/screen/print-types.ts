//! The values one print carries: its block identity, its palette, and its job.

import type { Lead } from "./print-plan";

/** One block the printer can commit: its stable identity. */
export interface PrintedBlock {
  readonly key: string;
}

/** The palette roles the printed page paints. */
export interface PrintPalette {
  readonly canvas: string;
  readonly panel: string;
  readonly border: string;
}

/** One print on its way to the terminal: the blocks it carries, and its rows. */
export interface PrintJob<Block extends PrintedBlock> {
  /** The session the blocks belong to. */
  readonly session: string | null;
  /** The blocks this print carries, frozen when it was measured. */
  readonly blocks: readonly Block[];
  /** The rows the print leads with. */
  readonly lead: Lead;
  /** The rows the surface measured, lead rows included; 0 before the measure. */
  readonly rows: number;
  /** The row the next chunk starts at. */
  readonly next: number;
}
