//! The print session: the surface, its book, and the frames that advance it.
//!
//! The hook owns when a print starts, how much it measures, and how its chunks
//! reach the terminal. It renders nothing: the page it is printing is a
//! separate component, so the scrollback mechanics and the painted rows can be
//! read apart.
//!
//! A step in flight is a print like any other. Its page paints the text the step
//! has streamed so far, the rows a later row has superseded are written while
//! the stream moves, and the edge keeps the rows it may still rewrite until the
//! stream passes them or the step commits. The rows are written once: the page
//! above the tray is the session's record.
//!
//! A print takes its rows from the window before it writes them. The footer
//! owns the terminal's last rows, so the rows a print needs can only come from
//! the footer giving them back: `onPrinted` is that hand-over, and it happens a
//! frame before the write. A write that lands while the footer still holds the
//! row scrolls the whole screen.

import { useRenderer } from "@opentui/react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { EDGE_ROWS, printRoom } from "../layout/footer-plan";
import { chunkRows, continues, leadRow, sealedRows } from "./print-plan";
import type { PrintJob, PrintedBlock, StepPrint } from "./print-types";
import { pinSurface, readOffset, replayWindow, windowSettled } from "./surface";

/** The blocks one print folds, so a burst is printed in bounded surfaces. */
const FOLD = 8;

/** How many times a print re-pins the surface before it replays the window. */
const MAX_PINS = 8;

/** Nothing printed yet. */
const NOTHING: ReadonlySet<string> = new Set();

/** What one session's print has already put in the terminal. */
interface Printed {
  /** The session the keys belong to; another session starts over. */
  readonly session: string | null;
  /** The blocks of that session the terminal already has. */
  readonly keys: ReadonlySet<string>;
  /** The prints of that session the terminal already has, streamed steps included. */
  readonly units: number;
}

/** Returns the surface and the print in flight for one session. */
export function usePrintSession<Block extends PrintedBlock>({
  session,
  blocks,
  step,
  onPrinted,
}: {
  /** The session the blocks belong to; a switch starts its book over. */
  session: string | null;
  /** The blocks waiting to print, in transcript order. */
  blocks: readonly Block[];
  /** The step in flight, or `null` when nothing streams. */
  step: StepPrint | null;
  /** Called with the rows a print takes, so the footer gives them back first. */
  onPrinted: (rows: number) => void;
}) {
  const renderer = useRenderer();
  const surface = useMemo(() => renderer.createScrollbackSurface(), [renderer]);
  useEffect(() => () => surface.destroy(), [surface]);
  // The blocks already in the terminal, and the job taking the next one. The
  // book is read while rendering, so it is a ref: a key that lands in it is
  // never handed to a second print in the same frame.
  const book = useRef<Printed>({ session: null, keys: NOTHING, units: 0 });
  const [currentJob, setCurrentJob] = useState<PrintJob<Block> | null>(null);
  // The frame that advances a print reads the job it is printing.
  const advancing = useRef<PrintJob<Block> | null>(null);
  advancing.current = currentJob;
  const pins = useRef(0);
  const replayed = useRef(false);
  /**
   * Records a print as written and lets the next one start.
   *
   * A print of a step in flight leaves no key behind: the block the step became
   * is the adapter's to recognise, and it hands the printer a list it has
   * already filtered. Every print takes one unit of the page, so the next one
   * leads with a separator like any other.
   */
  const settle = useCallback((done: PrintJob<Block>): void => {
    if (book.current.session === done.session) {
      book.current = {
        ...book.current,
        keys:
          done.step === null
            ? new Set([...book.current.keys, ...done.blocks.map((block) => block.key)])
            : book.current.keys,
        units: book.current.units + 1,
      };
    }
    pins.current = 0;
    replayed.current = false;
    setCurrentJob(null);
  }, []);
  /**
   * Writes the next chunk of one print.
   *
   * A block taller than the window reaches the terminal as a sequence of
   * bounded commits rather than as one the terminal cannot take. The chunk is
   * no taller than the rows above the footer, which is what keeps the print
   * from scrolling the whole screen. No chunk ends with a newline: the line the
   * print stops on is the line the edge starts on, and a newline there is a
   * blank row between the transcript and the step in flight.
   */
  const commitNext = useCallback(
    (current: PrintJob<Block>): void => {
      const rowsAbove = printRoom(renderer.terminalHeight, renderer.footerHeight);
      const to = Math.min(
        current.sealed,
        current.next + chunkRows(current.next, current.sealed, rowsAbove),
      );
      if (to <= current.next) {
        // A finished print with nothing left to write is done; a step in flight
        // is only waiting for rows.
        if (!current.open) {
          settle(current);
        }
        return;
      }
      // The footer holds the rows the print needs. It gives them back first, and
      // the frame that applies the new height writes the chunk.
      if (current.given < to) {
        onPrinted(to - current.given);
        setCurrentJob({ ...current, given: to });
        renderer.requestRender();
        return;
      }
      try {
        if (current.next > 0) {
          surface.render();
        }
        // A commit ends on a fresh line. The row after it is the row the edge
        // starts on, and a cursor left in the middle of the last row writes the
        // next row at that row's column as soon as the terminal reflowed.
        surface.commitRows(current.next, to);
      } catch {
        // The window changed between the render and the write. The print
        // measures itself again and the frames that follow write it at the
        // geometry the terminal has now; the rows already written keep the
        // width they were written at.
        surface.render();
        setCurrentJob({ ...current, rows: 0 });
        renderer.requestRender();
        return;
      }
      if (to >= current.rows) {
        settle(current);
      } else {
        setCurrentJob({ ...current, next: to });
      }
      renderer.requestRender();
    },
    [onPrinted, renderer, settle, surface],
  );
  /**
   * Measures the page and records how much of it may be written.
   *
   * A step in flight writes the rows its edge has sealed; every other print
   * writes all of them. A page that shrank as the stream re-read it cannot take
   * back a row the terminal already has, so the next row never moves behind the
   * rows that were written.
   */
  const measure = useCallback(
    (current: PrintJob<Block>): void => {
      surface.render();
      const rows = surface.height;
      const sealed = current.open ? sealedRows(rows, EDGE_ROWS) : rows;
      if (rows !== current.rows || sealed !== current.sealed) {
        setCurrentJob({ ...current, rows, sealed });
        return;
      }
      if (windowSettled(renderer)) {
        commitNext(current);
      }
    },
    [commitNext, renderer, surface],
  );

  // The renderer moves the surface to where its output started whenever a print
  // lands or the footer resizes, so the surface is held on the window's last row
  // for as long as it takes the frame that carries the move to be the last one.
  // A window that will not settle is replayed once per print rather than pinned
  // forever.
  useEffect(() => {
    const onFrame = (): void => {
      const want = Math.max(0, renderer.terminalHeight - renderer.footerHeight);
      const at = readOffset(renderer);
      if (at !== null && at !== want) {
        if (pins.current < MAX_PINS) {
          pins.current += 1;
          pinSurface(renderer);
          renderer.requestRender();
          return;
        }
        if (!replayed.current) {
          replayed.current = true;
          pins.current = 0;
          replayWindow(renderer);
          renderer.requestRender();
        }
        return;
      }
      pins.current = 0;
      replayed.current = false;
      const current = advancing.current;
      // The window is where it belongs and the footer has taken its rows: the
      // next chunk goes now.
      if (current !== null && current.next < current.sealed && windowSettled(renderer)) {
        commitNext(current);
      }
    };
    renderer.on("frame", onFrame);
    return () => {
      renderer.off("frame", onFrame);
    };
  }, [renderer, commitNext]);

  useLayoutEffect(() => {
    if (surface.isDestroyed) {
      return;
    }
    if (book.current.session !== session) {
      // Another session opens a page of its own, below the rows this one wrote:
      // the keys belong to the old session, the count of prints stays, so the
      // new session's first block still leads with the separator between them.
      book.current = { session, keys: NOTHING, units: book.current.units };
    }
    const current = currentJob;
    // A print belongs to the session that started it: another session opens its
    // own page, and the rows of the old one stay in the terminal.
    if (current !== null && current.session !== session) {
      pins.current = 0;
      replayed.current = false;
      setCurrentJob(null);
      return;
    }
    if (current === null) {
      // One print folds the blocks that are waiting, up to the fold: a burst of
      // commits does not become one surface the window cannot paint.
      const fold = blocks.filter((block) => !book.current.keys.has(block.key)).slice(0, FOLD);
      if (fold.length > 0) {
        pins.current = 0;
        replayed.current = false;
        setCurrentJob({
          session,
          blocks: fold,
          step: null,
          lead: leadRow(book.current.units),
          rows: 0,
          next: 0,
          given: 0,
          sealed: 0,
          open: false,
        });
        return;
      }
      if (step !== null) {
        pins.current = 0;
        replayed.current = false;
        setCurrentJob({
          session,
          blocks: [],
          step,
          lead: leadRow(book.current.units),
          rows: 0,
          next: 0,
          given: 0,
          sealed: 0,
          open: true,
        });
      }
      return;
    }
    // The step a print paints is over: its page is frozen, so its edge is final
    // and the rows it held are the step's last rows.
    if (current.step !== null && (step === null || !continues(current.step, step))) {
      if (current.rows === 0) {
        // The step ended before its page was measured. The page paints the
        // frozen step, so the rows it streamed are still the rows it prints.
        measure({ ...current, open: false });
        return;
      }
      if (current.open) {
        setCurrentJob({ ...current, open: false, sealed: current.rows });
        return;
      }
      if (windowSettled(renderer)) {
        commitNext(current);
      }
      return;
    }
    // The step painted new rows. The page already paints them, so the print
    // measures the page it has now, in the same frame the edge paints it: a
    // measure a frame behind the edge leaves the two rows between them in both.
    if (current.step !== null && step !== null && step !== current.step) {
      const painted = { ...current, step };
      setCurrentJob(painted);
      measure(painted);
      return;
    }
    measure(current);
  }, [blocks, commitNext, currentJob, measure, renderer, session, step, surface]);

  return { surface, job: currentJob };
}
