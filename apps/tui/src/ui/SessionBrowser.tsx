//! The session browser.
//!
//! The browser is a full-panel surface: it takes the keyboard from the composer
//! and covers the transcript, so a user is never typing into a screen they
//! cannot see. A narrow panel drops a table column instead of squeezing every
//! column into a fragment.

import { TextAttributes } from "@opentui/core";
import type { SessionSummariesDto } from "../bindings";
import type { Palette } from "../theme/palette";
import { fitColumns, renderHints, sessionCells } from "./format";

/** The keys the browser offers. */
export const BROWSER_HINTS = [
  { keys: "↑↓", action: "move" },
  { keys: "enter", action: "open" },
  { keys: "esc", action: "close" },
];

/** Renders the browser. */
export function SessionBrowser({
  summaries,
  error,
  selection,
  palette,
  width,
  height,
  now,
}: {
  summaries: SessionSummariesDto | null;
  error: string | null;
  selection: number;
  palette: Palette;
  width: number;
  height: number;
  now: number;
}) {
  const columns = fitColumns(Math.max(10, width - 6));
  const rows = summaries?.sessions ?? [];
  // A bounded read reports what it left out, so the title counts both halves
  // instead of passing a truncated list off as the whole one.
  const omitted = summaries?.omitted ?? 0;
  const title =
    summaries === null
      ? error === null
        ? "sessions · reading"
        : "sessions"
      : `sessions · ${rows.length}${omitted === 0 ? "" : ` +${omitted} more`}`;
  // The frame keeps its own two lines: the title and the key hints. The rest is
  // the window the selection scrolls inside.
  const visible = Math.max(1, height - 4);
  const offset = Math.max(0, Math.min(selection - visible + 1, rows.length - visible));
  const window = rows.slice(offset, offset + visible);
  return (
    <box flexDirection="column" flexGrow={1} paddingLeft={1} paddingRight={1}>
      <text fg={palette.ink} attributes={TextAttributes.BOLD}>
        {title}
      </text>
      {error === null ? null : (
        <text wrapMode="word" fg={palette.error}>
          {error}
        </text>
      )}
      {summaries !== null && rows.length === 0 ? (
        <text wrapMode="word" fg={palette.inkMuted}>
          no sessions yet; send a prompt to create one
        </text>
      ) : null}
      <box flexDirection="column" flexGrow={1}>
        {window.map((summary, index) => {
          const selected = offset + index === selection;
          const cells = sessionCells(summary, columns, now);
          return (
            <box key={summary.session_id} flexDirection="row" gap={2}>
              <text fg={selected ? palette.accent : palette.inkFaint}>{selected ? "›" : " "}</text>
              {cells.map((cell, cellIndex) => (
                <text
                  key={columns[cellIndex]?.header ?? cellIndex}
                  fg={selected ? palette.ink : palette.inkMuted}
                  attributes={cellIndex === 0 && selected ? TextAttributes.BOLD : undefined}
                >
                  {cell}
                </text>
              ))}
            </box>
          );
        })}
      </box>
      <text wrapMode="none" fg={palette.inkFaint}>
        {renderHints(width - 2, BROWSER_HINTS)}
      </text>
    </box>
  );
}
