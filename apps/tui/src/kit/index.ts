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
export {
  Pane,
  Indent,
  paneContentColumns,
  paneSlotRows,
  PANE_PADDING,
} from "./layout/Pane";
export { Hanging, markerColumns } from "./layout/Hanging";
export { Lines } from "./layout/Lines";
export { Runs, type Run } from "./layout/Runs";
export { Band, type BandItem, type BandPalette } from "./layout/Band";
export {
  Picker,
  pickerRows,
  type PickerItem,
  type PickerPalette,
} from "./layout/Picker";
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
  MIN_BLOCK_ROWS,
  type TextFieldPalette,
  type CursorWindow,
} from "./layout/TextField";
export { Hero } from "./layout/Hero";
export { heroLayout, HERO_LEAD_ROWS, HERO_ROWS, type HeroPlan } from "./layout/hero-plan";
export {
  bandColumnWidths,
  bandSlotRows,
  pickerSlotRows,
  MAX_BAND_ROWS,
  type BandColumnWidths,
} from "./layout/menu-band";
export { browserColumnWidths, type BrowserColumns } from "./layout/browser-columns";
export {
  Markdown,
  MarkdownBlock,
  MarkdownBlocks,
  type MarkdownPalette,
} from "./markdown";
export { splitBlocks, type StreamSplit } from "./markdown-split";
export {
  footerPlan,
  printRoom,
  CONTENT_INDENT,
  DETAIL_ROWS,
  LIVE_BAND_ROWS,
  FOOTER_MARGIN,
  FOOTER_MIN_COLUMNS,
  WELCOME_CHROME_ROWS,
  SESSION_CHROME_ROWS,
  type FooterLayout,
  type FooterMode,
} from "./layout/footer-plan";
export {
  browserPlan,
  browserDataRows,
  browserPanelColumns,
  MIN_PANEL_ROWS,
  MIN_CHAT_ROWS,
  DOCK_GAP_ROWS,
  BROWSER_CHROME_ROWS,
  BROWSER_FOOTER_ROWS,
  BROWSER_CHROME_COLUMNS,
  type BrowserLayout,
} from "./layout/dock-plan";
export { FooterFrame, type FooterPalette } from "./layout/FooterFrame";
export { Screen } from "./screen/Screen";
export { Printer, type PrintedBlock, type PrintPalette } from "./screen/Printer";
export { chunkRows, leadRow, type Lead } from "./screen/print-plan";
export { createSplitScreen, closeScreen } from "./screen/terminal";
export { pushScreen, windowRows } from "./screen/startup";
export { useTerminalTheme } from "./screen/use-terminal-theme";
