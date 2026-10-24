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
import { useEffect, useLayoutEffect, useMemo, useState } from "react";
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
  /** Called with the rows a commit added, so the footer can take back the rest. */
  onPrinted: (rows: number) => void;
}) {
  const renderer = useRenderer();
  const session = useStore(store, selectSessionId);
  const messages = useStore(store, selectTranscript);
  const expansions = useStore(store, selectExpansions);
  const surface = useMemo(() => renderer.createScrollbackSurface(), [renderer]);
  useEffect(() => () => surface.destroy(), [surface]);
  const [printed, setPrinted] = useState<Printed>({ session: null, keys: NOTHING });
  const known = printed.session === session ? printed.keys : NOTHING;
  const pending = useMemo(
    () => blocks(messages).filter((block) => !known.has(block.key)),
    [messages, known],
  );
  useLayoutEffect(() => {
    if (surface.isDestroyed || pending.length === 0) {
      return;
    }
    surface.render();
    if (surface.height > 0) {
      // No newline after the last row: a newline would leave the line below the
      // print empty, and that line is one of the rows the transcript shows. The
      // next commit starts on a line of its own regardless.
      surface.commitRows(0, surface.height, { trailingNewline: false });
    }
    onPrinted(surface.height);
    setPrinted({ session, keys: new Set([...known, ...pending.map((block) => block.key)]) });
  }, [surface, pending, onPrinted, session, known]);

  if (pending.length === 0) {
    return null;
  }
  return createPortal(
    <box flexDirection="column" width={surface.width} backgroundColor={palette.canvas}>
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
