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
//! The rows the app prints carry the canvas, so the screen reads as one app and
//! never as the terminal behind it; the app never pads a short transcript, so
//! the history holds only the rows the session actually said.

import { createPortal, useRenderer } from "@opentui/react";
import { useEffect, useLayoutEffect, useMemo, useState } from "react";
import { selectExpansions, selectSessionId, selectTranscript } from "../app/select";
import type { AppStore } from "../app/store";
import type { Palette } from "../theme/palette";
import { Fill } from "./Fill";
import { PANEL_MARGIN } from "./panel";
import { blocks, CommittedBlock } from "./Transcript";
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

/** The rows the page under a print is painted for; the surface clips the rest. */
const PRINTED_ROWS = 400;

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

/** Requests a frame once the renderer's split output queue has drained. */
function waitForOutput(
  renderer: ReturnType<typeof useRenderer>,
  later: (ms: number, done: () => void) => void,
  done: () => void,
): void {
  const queue = (renderer as unknown as { externalOutputQueue?: { size: number } })
    .externalOutputQueue;
  let tries = 0;
  const tick = () => {
    tries += 1;
    if (queue === undefined || queue.size === 0 || tries > 40) {
      done();
      return;
    }
    later(25, tick);
  };
  later(25, tick);
}

/**
 * Prints the committed blocks the window has not printed yet.
 *
 * Nothing but the blocks is printed: the rows a short transcript leaves are the
 * footer's, so the history holds what the reader said and what the daemon
 * answered, never a run of blank rows that would sit above the first question
 * for the rest of the session.
 *
 * A session switch prints the transcript it opens under the one before it: the
 * rows a reader scrolled through are the terminal's, and nothing the application
 * can do takes them back.
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
  /** Called with the rows a commit added, so the footer takes back the rest. */
  onPrinted: (rows: number) => void;
}) {
  const renderer = useRenderer();
  const session = useStore(store, selectSessionId);
  const messages = useStore(store, selectTranscript);
  const expansions = useStore(store, selectExpansions);
  const surface = useMemo(() => renderer.createScrollbackSurface(), [renderer]);
  useEffect(() => () => surface.destroy(), [surface]);
  // The renderer moves the surface to where its output started whenever a print
  // lands or the footer resizes, so the surface is held on the window's last row
  // for as long as it takes the frame that carries the move to be the last one.
  useEffect(() => {
    let stuck = 0;
    const onFrame = (): void => {
      const want = Math.max(0, renderer.terminalHeight - renderer.footerHeight);
      const at = readOffset(renderer);
      if (at === null || at === want) {
        stuck = 0;
        return;
      }
      stuck += 1;
      if (stuck > 4) {
        return;
      }
      pinSurface(renderer);
      renderer.requestRender();
    };
    renderer.on("frame", onFrame);
    return () => {
      renderer.off("frame", onFrame);
    };
  }, [renderer]);
  const [printed, setPrinted] = useState<Printed>({ session: null, keys: NOTHING });
  const known = printed.session === session ? printed.keys : NOTHING;
  const pending = useMemo(
    () => blocks(messages).filter((block) => !known.has(block.key)),
    [messages, known],
  );
  const [reserved, setReserved] = useState<string | null>(null);
  useLayoutEffect(() => {
    if (surface.isDestroyed || pending.length === 0) {
      return;
    }
    const timers: Array<ReturnType<typeof setTimeout>> = [];
    const later = (ms: number, done: () => void): void => {
      timers.push(setTimeout(done, ms));
    };
    const key = pending.map((block) => block.key).join(",");
    surface.render();
    if (surface.height < 1) {
      setPrinted({ session, keys: new Set([...known, ...pending.map((block) => block.key)]) });
      return;
    }
    if (reserved !== key) {
      // The window gives the rows back before the print takes them: a region
      // that shrinks after its rows have been written leaves the surface where
      // the output started, and the rows it vacates stay painted over.
      setReserved(key);
      onPrinted(surface.height);
      return;
    }
    surface.render();
    // No newline after the last row: a newline would leave the line below the
    // print empty, and that line is one of the rows the transcript shows. The
    // next commit starts on a line of its own regardless.
    surface.commitRows(0, surface.height, { trailingNewline: false });
    setPrinted({ session, keys: new Set([...known, ...pending.map((block) => block.key)]) });
    setReserved(null);
    // The renderer moves the surface to where its output started; the frames
    // after the print are the ones that put the footer back on the last row.
    waitForOutput(renderer, later, () => {
      if (surface.isDestroyed) {
        return;
      }
      pinSurface(renderer);
      renderer.requestRender();
    });
    return () => {
      for (const timer of timers) {
        clearTimeout(timer);
      }
    };
  }, [renderer, surface, pending, onPrinted, reserved, session, known]);

  if (pending.length === 0) {
    return null;
  }
  // The print continues the panel the footer closes: its rows carry the frame's
  // sides, and the first print of a session carries the frame's top edge. The
  // page under them is painted a row at a time, so no row of the transcript is
  // the terminal's own background.
  const frameColumns = surface.width - PANEL_MARGIN * 2;
  const innerColumns = frameColumns - 2;
  const first = known.size === 0;
  const closing = printed.session !== null && printed.session !== session;
  return createPortal(
    <box flexDirection="column" width={surface.width}>
      <box position="absolute" top={0} left={0} width={surface.width} height={PRINTED_ROWS}>
        <Fill rows={PRINTED_ROWS} columns={surface.width} color={palette.canvas} />
      </box>
      {closing ? (
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
          border={first ? ["top", "left", "right"] : ["left", "right"]}
          borderColor={palette.border}
          paddingLeft={1}
          paddingRight={1}
        >
          <box position="absolute" top={0} left={0} width={innerColumns} height={PRINTED_ROWS}>
            <Fill rows={PRINTED_ROWS} columns={innerColumns} color={palette.panel} />
          </box>
          {pending.map((block, index) => (
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
