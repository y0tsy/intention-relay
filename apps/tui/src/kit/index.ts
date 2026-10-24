//! The TUI kernel: the only entry a surface imports from.
//!
//! A UI module imports `../kit` and nothing deeper: the layout arithmetic, the
//! text measurement, and the screen's imperative code live behind these handles.

export {
  displayWidth,
  markerText,
  pad,
  truncate,
  cellText,
  cellTextRight,
  wrapRows,
  takeRows,
} from "./text";
export { useWindow, type Window } from "./window";
export {
  ContentColumns,
  SlotRows,
  PrintedRows,
  useContentColumns,
  useSlotRows,
  usePrintedRows,
} from "./layout/context";
export { Stack, Row, Spacer } from "./layout/Stack";
export { Surface } from "./layout/Surface";
export { Center } from "./layout/Center";
export { Rule } from "./layout/Rule";
export { Badge } from "./layout/Badge";
export { Pane, Indent, paneContentColumns, paneSlotRows, PANE_PADDING } from "./layout/Pane";
export { Hanging, markerColumns } from "./layout/Hanging";
export { Lines } from "./layout/Lines";
export {
  Table,
  type TableColumn,
  type TableRow,
  type TableCell,
  type TableCellStyle,
} from "./layout/Table";
export { Cell } from "./layout/Cell";
export { scrollWindow, type ScrollWindow } from "./layout/ScrollWindow";
export {
  TextField,
  inputSlotRows,
  cursorWindow,
  locateCaret,
  windowStart,
  INPUT_CHROME_ROWS,
  INPUT_CURSOR,
  INPUT_PROMPT,
  INPUT_CONTINUATION,
  type TextFieldPalette,
  type CursorWindow,
} from "./layout/TextField";
export { Hero } from "./layout/Hero";
export { heroLayout, HERO_LEAD_ROWS, HERO_ROWS, type HeroPlan } from "./layout/hero-plan";
export { bandSlotRows, pickerSlotRows, MAX_BAND_ROWS } from "./layout/menu-band";
export { browserColumnWidths, type BrowserColumns } from "./layout/browser-columns";
