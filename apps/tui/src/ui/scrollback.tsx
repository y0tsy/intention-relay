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
//! never as the terminal behind it; and a transcript too short to fill the window
//! is padded with canvas rows, so the footer stays on the window's last row and
//! no hole is left below it.

import { createPortal, useRenderer } from "@opentui/react";
import { type RefObject, useEffect, useLayoutEffect, useMemo, useState } from "react";
import { selectExpansions, selectSessionId, selectTranscript } from "../app/select";
import type { AppStore } from "../app/store";
import type { Palette } from "../theme/palette";
import { Fill } from "./Fill";
import { CONTENT_INDENT } from "./panel";
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

/** The rows the app has committed into the terminal's scrollback. */
function useCommitted(): [number, (grow: number) => void] {
  const [count, setCount] = useState(0);
  return [count, (grow: number) => setCount((before) => before + grow)];
}

/**
 * Prints the committed blocks the window has not printed yet, padding the
 * output when it would leave the footer floating above the window's last row.
 *
 * `footerRows` is the footer's own height from the footer's own effect, which
 * runs first in the same commit, so the padding always matches the window the
 * rows are printed onto.
 *
 * A session switch prints the transcript it opens under the one before it: the
 * rows a reader scrolled through are the terminal's, and nothing the application
 * can do takes them back.
 */
export function Printer({
  store,
  palette,
  width,
  terminalHeight,
  footerRows,
}: {
  store: AppStore;
  palette: Palette;
  width: number;
  terminalHeight: number;
  footerRows: RefObject<number>;
}) {
  const renderer = useRenderer();
  const session = useStore(store, selectSessionId);
  const messages = useStore(store, selectTranscript);
  const expansions = useStore(store, selectExpansions);
  const surface = useMemo(() => renderer.createScrollbackSurface(), [renderer]);
  useEffect(() => () => surface.destroy(), [surface]);
  const [printed, setPrinted] = useState<Printed>({ session: null, keys: NOTHING });
  const [committed, addCommitted] = useCommitted();
  const [filler, setFiller] = useState(0);
  const known = printed.session === session ? printed.keys : NOTHING;
  const pending = useMemo(
    () => blocks(messages).filter((block) => !known.has(block.key)),
    [messages, known],
  );
  // biome-ignore lint/correctness/useExhaustiveDependencies: footerRows is a ref; its .current the footer's own effect writes first in the same commit.
  useLayoutEffect(() => {
    if (surface.isDestroyed || (pending.length === 0 && filler === 0)) {
      return;
    }
    // The padding is a row range of its own, so it is laid out before it is
    // committed: a block never lands above the blank rows that follow it.
    const needed = Math.max(0, terminalHeight - (footerRows.current ?? 0) - committed);
    if (needed > filler) {
      setFiller(needed);
      return;
    }
    surface.render();
    if (surface.height > 0) {
      // No newline after the last row: a newline would leave the line below the
      // print empty, and that line is one of the rows the transcript shows. The
      // next commit starts on a line of its own regardless.
      surface.commitRows(0, surface.height, { trailingNewline: false });
    }
    addCommitted(surface.height);
    // The rows just written can defer the footer's own height: a frame after
    // them is what applies it, and without one the stale region would keep the
    // window open below the footer.
    renderer.requestRender();
    if (filler > 0) {
      setFiller(0);
    }
    if (pending.length > 0) {
      setPrinted({ session, keys: new Set([...known, ...pending.map((block) => block.key)]) });
    }
  }, [surface, renderer, pending, filler, committed, terminalHeight, session, known]);

  if (pending.length === 0 && filler === 0) {
    return null;
  }
  return createPortal(
    <box flexDirection="column" width={surface.width} backgroundColor={palette.canvas}>
      {filler === 0 ? null : <Fill rows={filler} columns={surface.width} palette={palette} />}
      {pending.map((block) => (
        <box key={block.key} flexDirection="column">
          <box paddingLeft={CONTENT_INDENT}>
            <CommittedBlock
              block={block}
              palette={palette}
              width={width}
              expansion={expansions.get(block.key) ?? 0}
            />
          </box>
          {/* One blank row closes a block: the row the panel kept between two,
              painted as a row of the page so it is a page and not a hole. */}
          <Fill rows={1} columns={surface.width} palette={palette} />
        </box>
      ))}
    </box>,
    surface.root,
    null,
  );
}
