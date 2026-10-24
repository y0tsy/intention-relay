//! The input's command hint band: the commands the first word selects, or the
//! values of the argument word the caret sits in.
//!
//! The band is one region of the chat panel directly above the input block, and
//! the transcript gives up exactly the rows the band takes. The kernel's `Band`
//! owns the frame, the columns, the window, and the overflow row; this module
//! names the registry's words and the titles the two kinds carry.

import type { CommandMenu } from "../app/menu";
import { MAX_COMMAND_ROWS, menuLabels, menuTrailing } from "../app/menu";
import { Band } from "../kit";
import type { Palette } from "../theme/palette";
import { CURSOR_MARKER, ROW_MARKER } from "./glyphs";

/** Renders the band the word the caret sits in opens. */
export function HintBand({
  menu,
  highlight,
  palette,
}: {
  menu: CommandMenu;
  /** The row the reader is on; the band shows the window around it. */
  highlight: number;
  palette: Palette;
}) {
  return (
    <Band
      items={menu.rows.map((row) => ({
        key: row.label,
        label: row.label,
        description: row.description,
        trailing: row.trailing,
      }))}
      highlight={highlight}
      labels={menuLabels(menu)}
      trailing={menuTrailing(menu)}
      title={menu.kind === "command" ? "Commands" : "Arguments"}
      noun={menu.kind === "command" ? "command" : "value"}
      limit={MAX_COMMAND_ROWS}
      marker={CURSOR_MARKER}
      rowMarker={ROW_MARKER}
      palette={palette}
    />
  );
}
