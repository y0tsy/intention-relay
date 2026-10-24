//! The browser's window: the rows the panel shows, and how wide they stand.
//!
//! The table owns the slice of the session list the scroll window shows, so
//! the body can compose the frame without learning the cursor arithmetic.

import type { SessionSummaryDto } from "../../bindings";
import {
  browserColumnWidths,
  browserDataRows,
  scrollWindow,
  useContentColumns,
  useSlotRows,
} from "../../kit";

/** The rows one browser window shows, and the columns they stand in. */
export interface BrowserTable {
  readonly shown: readonly SessionSummaryDto[];
  /** The index of `shown[0]` in the whole list. */
  readonly start: number;
  readonly first: number;
  readonly last: number;
  readonly columns: readonly { key: string; width: number }[];
}

/** Returns the window of sessions the browser shows for one selection. */
export function useBrowserTable(
  rows: readonly SessionSummaryDto[],
  selection: number,
): BrowserTable {
  const columns = useContentColumns();
  const widths = browserColumnWidths(columns);
  const dataRows = browserDataRows(useSlotRows(0));
  const window = scrollWindow({ count: rows.length, visible: dataRows, selected: selection });
  return {
    shown: rows.slice(window.start, window.end),
    start: window.start,
    first: window.first,
    last: window.last,
    columns: [
      { key: "modified", width: widths.modified },
      { key: "created", width: widths.created },
      { key: "size", width: widths.size },
      { key: "title", width: widths.title },
      { key: "path", width: widths.path },
    ],
  };
}
