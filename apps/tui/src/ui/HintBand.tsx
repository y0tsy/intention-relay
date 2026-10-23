//! The keys this surface offers, as one line that never cuts a hint.

import type { Palette } from "../theme/palette";
import { renderHints } from "./format";

/**
 * Renders the hint band for one width.
 *
 * The width is the band's own, so the two columns of padding come out of the
 * budget before the hints are fitted: a hint that fits the box but not the text
 * area inside it is a hint cut mid-word, which is the one thing the band must
 * never do.
 */
export function HintBand({ width, palette }: { width: number; palette: Palette }) {
  return (
    <box paddingLeft={1} paddingRight={1}>
      <text wrapMode="none" fg={palette.inkFaint}>
        {renderHints(width - 2)}
      </text>
    </box>
  );
}
