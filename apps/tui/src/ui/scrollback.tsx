//! Printing the finished transcript into the terminal's own scrollback.
//!
//! The window keeps only the live footer; every block that commits is printed
//! above it once, as rows the terminal owns. Those rows are the window's own
//! history: the terminal's wheel and its scrollbar move through them, and a drag
//! selects them, because the application never paints them and never asks the
//! terminal for the mouse.
//!
//! A block reaches the terminal through a scrollback surface: a render context
//! that is not on screen. The block is mounted into that surface from the
//! application's own tree, so the surface paints the very components the live
//! tail used and a step does not change appearance when it commits; the rows it
//! painted are then committed, and the surface forgets them and takes the next
//! block.
//!
//! One print is a bounded fold of blocks. A burst of commits prints one surface
//! after another instead of mounting the whole burst into one surface: the rows
//! of a print are the rows the window asked the footer to give back, and a print
//! taller than the window is committed in chunks that each fit above the footer,
//! so no single call carries more rows than the terminal can take.
//!
//! The rows the app prints carry the canvas, so the screen reads as one app and
//! never as the terminal behind it; the app never pads a short transcript, so
//! the history holds only the rows the session actually said.

import { createPortal, useRenderer } from "@opentui/react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { selectExpansions, selectSessionId, selectTranscript } from "../app/select";
import type { AppStore } from "../app/store";
import type { Palette } from "../theme/palette";
import { Fill } from "./Fill";
import { PANEL_MARGIN, printRoom } from "./panel";
import { chunkRows, type Lead, leadRow } from "./print-plan";
import { type Block, blocks, CommittedBlock } from "./Transcript";
import { useStore } from "./use-store";

/** What one session's print has already put in the terminal. */
interface Printed {
  /** The session the keys belong to; another session starts over. */
  readonly session: string | null;
  /** The blocks of that session the terminal already has. */
  readonly keys: ReadonlySet<string>;
}

/** Nothing printed yet. */
const NOTHING: ReadonlySet<string> = new Set();

/** One print on its way to the terminal: the blocks it carries, and its rows. */
interface Job {
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

/** The blocks one print folds, so a burst is printed in bounded surfaces. */
const FOLD = 8;

/** The rows the page under a print is painted for before the surface measures it. */
const PAGE_ROWS = 400;

/** How many times a print re-pins the surface before it replays the window. */
const MAX_PINS = 8;

/**
 * Puts the split surface back on the window's last row.
 *
 * A commit leaves the surface where its output started, so the region would sit
 * as many rows above the bottom as were just printed and the rows below it stay
 * the terminal's own background. The pinned offset is the one the renderer uses
 * for a frame it presents on its own.
 */
function pinSurface(renderer: ReturnType<typeof useRenderer>): void {
  const surface = renderer as unknown as {
    setRenderOffset?: (offset: number) => void;
    terminalHeight: number;
    footerHeight: number;
  };
  const offset = Math.max(0, surface.terminalHeight - surface.footerHeight);
  try {
    surface.setRenderOffset?.(offset);
  } catch {
    // A pin that lands while the terminal is busy dies quietly: the frame it
    // follows heals the surface on its own repaint.
  }
}

/** Returns the surface's row, or null when the renderer keeps it out of reach. */
function readOffset(renderer: ReturnType<typeof useRenderer>): number | null {
  const at = (renderer as unknown as { renderOffset?: number }).renderOffset;
  return typeof at === "number" ? at : null;
}

/**
 * Reports whether the window has taken the rows a print gave it back.
 *
 * The renderer applies a footer's new rows on a frame of its own, and only when
 * the split output queue has drained: a print written before that frame is
 * written above a footer that still holds the rows, which scrolls the whole
 * screen and leaves a copy of the footer the scroll moved.
 */
function windowSettled(renderer: ReturnType<typeof useRenderer>): boolean {
  const at = renderer as unknown as {
    pendingNativeMode?: unknown;
    externalOutputQueue?: { size: number };
  };
  return (at.pendingNativeMode ?? null) === null && (at.externalOutputQueue?.size ?? 0) === 0;
}

/**
 * Clears the window without touching the rows above it.
 *
 * `resetSplitFooterForReplay` is the renderer's own way back to a known screen:
 * it homes the cursor, erases what is on screen, and leaves the saved lines
 * alone, which is what a window whose rows have drifted needs.
 */
function replayWindow(renderer: ReturnType<typeof useRenderer>): void {
  const surface = renderer as unknown as {
    resetSplitFooterForReplay?: (options: { clearSavedLines?: boolean }) => void;
  };
  try {
    surface.resetSplitFooterForReplay?.({ clearSavedLines: false });
  } catch {
    // A window that cannot be replayed is repainted by the frames that follow.
  }
}

/**
 * Prints the committed blocks the window has not printed yet.
 *
 * A print takes the blocks that are waiting, up to the fold, and commits them in
 * chunks no taller than the rows the window gave back. Each chunk waits for the
 * frame that applies those rows and puts the surface back on the window's last
 * row: a print written into a footer that still holds the rows moves the footer
 * up, and nothing paints the copy the scroll leaves.
 */
export function Printer({
  store,
  palette,
  width,
  onPrinted,
}: {
  store: AppStore;
  palette: Palette;
  width: number;
  /** Called with the rows a print takes, so the footer gives them back first. */
  onPrinted: (rows: number) => void;
}) {
  const renderer = useRenderer();
  const session = useStore(store, selectSessionId);
  const messages = useStore(store, selectTranscript);
  const expansions = useStore(store, selectExpansions);
  const surface = useMemo(() => renderer.createScrollbackSurface(), [renderer]);
  useEffect(() => () => surface.destroy(), [surface]);
  const transcript = useMemo(() => blocks(messages), [messages]);
  // The blocks already in the terminal, and the job taking the next one. The
  // book is read while rendering, so it is a ref: a key that lands in it is
  // never handed to a second print in the same frame.
  const book = useRef<Printed>({ session: null, keys: NOTHING });
  const [currentJob, setCurrentJob] = useState<Job | null>(null);
  // The frame that advances a print reads the job it is printing.
  const advancing = useRef<Job | null>(null);
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
  const settle = useCallback((done: Job): void => {
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
   * from scrolling the whole screen: a chunk written into a footer that still
   * holds those rows moves the footer up, and the copy the scroll leaves is
   * never painted again.
   */
  const commitNext = useCallback(
    (current: Job): void => {
      const room = printRoom(renderer.terminalHeight, renderer.footerHeight);
      const to = current.next + chunkRows(current.next, current.rows, room);
      if (current.next > 0) {
        surface.render();
      }
      // No newline after the last row: a newline would leave the line below the
      // print empty, and that line is one of the rows the transcript shows. The
      // next commit starts on a line of its own regardless.
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
      const fold = transcript.filter((block) => !book.current.keys.has(block.key)).slice(0, FOLD);
      if (fold.length === 0) {
        return;
      }
      pins.current = 0;
      replayed.current = false;
      // The blocks are mounted on the render this call asks for and measured on
      // the pass after it, when the surface holds them.
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
      // The window gives the rows back before the print takes them: a region
      // that shrinks after its rows have been written leaves the surface where
      // the output started, and the rows it vacates stay painted over.
      onPrinted(surface.height);
      setCurrentJob({ ...current, rows: surface.height });
      return;
    }
    // The chunks of the print are written by the frames the window settles on,
    // which is what applies the rows the print took and puts the surface back on
    // the window's last row. A window that is not there yet waits.
    if (windowSettled(renderer)) {
      commitNext(current);
    }
  }, [renderer, surface, session, transcript, onPrinted, currentJob, settle, commitNext]);

  const current = currentJob;
  if (current === null) {
    return null;
  }
  // The print continues the panel the footer closes: its rows carry the frame's
  // sides, the first print of a session carries the frame's top edge, and a
  // session switch closes the frame above it first. The page under them is
  // painted a row at a time, so no row of the transcript is the terminal's own
  // background, and it is exactly as tall as the print.
  const rows = current.rows > 0 ? current.rows : PAGE_ROWS;
  const frameColumns = surface.width - PANEL_MARGIN * 2;
  const innerColumns = frameColumns - 2;
  return createPortal(
    <box flexDirection="column" width={surface.width}>
      <box position="absolute" top={0} left={0} width={surface.width} height={rows}>
        <Fill rows={rows} columns={surface.width} color={palette.canvas} />
      </box>
      {current.lead.closing ? (
        <box flexDirection="row" height={1}>
          <box width={PANEL_MARGIN} />
          <box
            width={frameColumns}
            borderStyle="rounded"
            border={["bottom"]}
            borderColor={palette.border}
          />
        </box>
      ) : null}
      <box flexDirection="row">
        <box width={PANEL_MARGIN} />
        <box
          width={frameColumns}
          flexDirection="column"
          borderStyle="rounded"
          border={current.lead.top ? ["top", "left", "right"] : ["left", "right"]}
          borderColor={palette.border}
          paddingLeft={1}
          paddingRight={1}
        >
          <box position="absolute" top={0} left={0} width={innerColumns} height={rows}>
            <Fill rows={rows} columns={innerColumns} color={palette.panel} />
          </box>
          {current.lead.separator ? <box height={1} /> : null}
          {current.blocks.map((block, index) => (
            <box key={block.key} flexDirection="column">
              {index === 0 ? null : <box height={1} />}
              <CommittedBlock
                block={block}
                palette={palette}
                width={width}
                expansion={expansions.get(block.key) ?? 0}
              />
            </box>
          ))}
        </box>
        <box width={PANEL_MARGIN} />
      </box>
    </box>,
    surface.root,
    null,
  );
}
