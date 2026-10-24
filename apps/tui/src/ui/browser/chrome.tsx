//! The browser's chrome: its tabs, its search bar, and its legend.
//!
//! The chrome names where the reader is, not how many sessions there are: the
//! table owns the rows, so a change to the legend never touches the cursor.

import { TextAttributes } from "@opentui/core";
import { BROWSER_TABS, type BrowserTab, tabLabel } from "../../app/browser";
import { Cell, Row } from "../../kit";
import type { Palette } from "../../theme/palette";
import { ACTIVE_TAB, INACTIVE_TAB, SEARCH_PLACEHOLDER } from "../glyphs";

/** The keys the browser offers, as the legend spells them. */
export const BROWSER_HINTS = [
  { keys: "Enter", action: "select" },
  { keys: "Tab", action: "switch tab" },
  { keys: "Esc", action: "close" },
];

/** Returns the tab radio row. */
export function TabRow({ tab, palette }: { tab: BrowserTab; palette: Palette }) {
  return (
    <text wrapMode="none" bg={palette.headerBg}>
      <span> </span>
      {BROWSER_TABS.map((entry) => {
        const active = entry === tab;
        return (
          <span key={entry}>
            {entry === BROWSER_TABS[0] ? null : <span fg={palette.inkFaint}>{"  |  "}</span>}
            <span fg={active ? palette.accent : palette.inkFaint}>
              {`${active ? ACTIVE_TAB : INACTIVE_TAB} `}
            </span>
            <span
              fg={active ? palette.ink : palette.inkMuted}
              attributes={active ? TextAttributes.BOLD : undefined}
            >
              {tabLabel(entry)}
            </span>
          </span>
        );
      })}
    </text>
  );
}

/** Returns the search bar, which shows the filter or its placeholder. */
export function SearchBar({ filter, palette }: { filter: string; palette: Palette }) {
  const empty = filter.length === 0;
  return (
    <Cell
      text={empty ? SEARCH_PLACEHOLDER : filter}
      ink={empty ? palette.inkFaint : palette.ink}
      background={palette.inputBg}
    />
  );
}

/** Returns the footer: the legend, and the counter of the rows the window shows. */
export function BrowserFooter({
  first,
  last,
  total,
  palette,
}: {
  first: number;
  last: number;
  total: number;
  palette: Palette;
}) {
  const counter = total === 0 ? "0 of 0" : `${first}-${last} of ${total}`;
  return (
    <Row background={palette.panel}>
      <Row grow={1}>
        {BROWSER_HINTS.map((hint, index) => (
          <text key={hint.keys} wrapMode="none">
            {index === 0 ? null : <span fg={palette.inkFaint}>{"  "}</span>}
            <span fg={palette.inkMuted}>{hint.keys}</span>
            <span fg={palette.inkFaint}>{` ${hint.action}`}</span>
          </text>
        ))}
      </Row>
      <text wrapMode="none" fg={palette.inkFaint}>
        {counter}
      </text>
    </Row>
  );
}
