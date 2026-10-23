//! Copying the text the reader selected.
//!
//! The renderer owns the selection: it starts one on a mouse down over selectable
//! text, follows the drag, and reports the finished selection once the button is
//! released. The app's part is the last step - putting that text where the reader
//! can paste it - and the clipboard service writes to the host clipboard and to
//! the terminal's own (OSC 52), because a terminal may offer one, the other, or
//! neither.

import {
  createClipboard,
  createHostClipboard,
  createRendererClipboardAdapter,
} from "@opentui/core";
import { useRenderer, useSelectionHandler } from "@opentui/react";
import { useMemo } from "react";

/** Puts one selected text where the reader can paste it. */
export type CopyText = (text: string) => void;

/**
 * Copies every finished text selection.
 *
 * `write` names the destination instead, which a test uses to read the text
 * without touching the machine's own clipboard.
 */
export function useCopyOnSelect(write?: CopyText): void {
  const renderer = useRenderer();
  const copy = useMemo<CopyText>(() => {
    if (write !== undefined) {
      return write;
    }
    const clipboard = createClipboard({
      host: createHostClipboard(),
      terminal: createRendererClipboardAdapter(renderer),
    });
    return (text) => {
      void clipboard.writeText(text, { destination: "best-available" });
    };
  }, [renderer, write]);
  useSelectionHandler((selection) => {
    const text = selection.getSelectedText();
    if (text.length > 0) {
      copy(text);
    }
  });
}
