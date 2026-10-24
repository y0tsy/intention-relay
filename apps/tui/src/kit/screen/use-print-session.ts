//! The print session: the surface, its book, and the frames that advance it.
//!
//! The hook owns when a print starts, how much it measures, and how its chunks
//! reach the terminal. It renders nothing: the page it is printing is a
//! separate component, so the scrollback mechanics and the painted rows can be
//! read apart.

import { useRenderer } from "@opentui/react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { printRoom } from "../layout/footer-plan";
import { chunkRows, leadRow } from "./print-plan";
import type { PrintJob, PrintedBlock } from "./print-types";
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
}

/** Returns the surface and the print in flight for one session. */
export function usePrintSession<Block extends PrintedBlock>({
  session,
  blocks,
  onPrinted,
}: {
  /** The session the blocks belong to; a switch closes the frame above them. */
  session: string | null;
  /** The blocks waiting to print, in transcript order. */
  blocks: readonly Block[];
  /** Called with the rows a print takes, so the footer gives them back first. */
  onPrinted: (rows: number) => void;
}) {
  const renderer = useRenderer();
  const surface = useMemo(() => renderer.createScrollbackSurface(), [renderer]);
  useEffect(() => () => surface.destroy(), [surface]);
  // The blocks already in the terminal, and the job taking the next one. The
  // book is read while rendering, so it is a ref: a key that lands in it is
  // never handed to a second print in the same frame.
  const book = useRef<Printed>({ session: null, keys: NOTHING });
  const [currentJob, setCurrentJob] = useState<PrintJob<Block> | null>(null);
  // The frame that advances a print reads the job it is printing.
  const advancing = useRef<PrintJob<Block> | null>(null);
  advancing.current = currentJob;
  // A session switch closes the frame the previous session left open, and that
  // edge belongs to the first print after the switch even when a commit of the
  // previous session lands in between.
  const reopen = useRef(false);
  const pins = useRef(0);
  const replayed = useRef(false);
  /**
   * Records a block as printed and lets the next one start.
   *
   * A block of the session that is gone keeps its own book: the new session
   * starts from nothing, and the closing edge is what joins the two. The job is
   * state, never a ref, so the print moving forward is a render the surface's own
   * frame carries.
   */
  const settle = useCallback((done: PrintJob<Block>): void => {
    if (book.current.session === done.session) {
      book.current = {
        session: done.session,
        keys: new Set([...book.current.keys, ...done.blocks.map((block) => block.key)]),
      };
    }
    pins.current = 0;
    replayed.current = false;
    setCurrentJob(null);
  }, []);
  /**
   * Commits the next chunk of one print.
   *
   * A block taller than the window reaches the terminal as a sequence of
   * bounded commits rather than as one the terminal cannot take. The chunk is
   * no taller than the rows above the footer, which is what keeps the print
   * from scrolling the whole screen.
   */
  const commitNext = useCallback(
    (current: PrintJob<Block>): void => {
      const rowsAbove = printRoom(renderer.terminalHeight, renderer.footerHeight);
      const to = current.next + chunkRows(current.next, current.rows, rowsAbove);
      if (current.next > 0) {
        surface.render();
      }
      // No newline after the last row: a newline would leave the line below the
      // print empty, and that line is one of the rows the transcript shows.
      surface.commitRows(current.next, to, { trailingNewline: to < current.rows });
      if (to >= current.rows) {
        settle(current);
      } else {
        setCurrentJob({ ...current, next: to });
      }
      renderer.requestRender();
    },
    [renderer, surface, settle],
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
      // The window is where it belongs, its rows are the window's, and the
      // footer has taken the rows the print gave back: the next chunk goes now.
      if (
        current !== null &&
        current.rows > 0 &&
        current.next < current.rows &&
        windowSettled(renderer)
      ) {
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
      reopen.current = book.current.session !== null;
      book.current = { session, keys: NOTHING };
    }
    const current = currentJob;
    if (current === null) {
      // One print folds the blocks that are waiting, up to the fold: a burst of
      // commits does not become one surface the window cannot paint.
      const fold = blocks.filter((block) => !book.current.keys.has(block.key)).slice(0, FOLD);
      if (fold.length === 0) {
        return;
      }
      pins.current = 0;
      replayed.current = false;
      setCurrentJob({
        session,
        blocks: fold,
        lead: leadRow(book.current.keys.size, reopen.current),
        rows: 0,
        next: 0,
      });
      reopen.current = false;
      return;
    }
    surface.render();
    if (current.rows === 0) {
      if (surface.height < 1) {
        settle(current);
        return;
      }
      // The window gives the rows back before the print takes them.
      onPrinted(surface.height);
      setCurrentJob({ ...current, rows: surface.height });
      return;
    }
    // The chunks of the print are written by the frames the window settles on.
    if (windowSettled(renderer)) {
      commitNext(current);
    }
  }, [renderer, surface, session, blocks, onPrinted, currentJob, settle, commitNext]);

  return { surface, job: currentJob };
}
