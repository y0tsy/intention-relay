//! The picker panel: one framed row per choice, with the cursor and the legend.
//!
//! The panel owns its frame, its row markers, its selected wash, and its legend;
//! a surface names the choices and the one the reader is on.

import { TextAttributes } from "@opentui/core";
import { displayWidth, truncate } from "../text";
import { useContentColumns } from "./context";
import { Pane, paneSlotRows } from "./Pane";

/** One choice the picker shows. */
export interface PickerItem {
  /** A stable identity for the row. */
  readonly key: string;
  /** The choice's own name. */
  readonly label: string;
  /** The one-line explanation beside it. */
  readonly detail: string;
  /** Whether this is the choice in force. */
  readonly current: boolean;
}

/** The palette roles the picker paints. */
export interface PickerPalette {
  readonly accent: string;
  readonly accentDeep: string;
  readonly headerBg: string;
  readonly rowSelectedBg: string;
  readonly ink: string;
  readonly inkMuted: string;
  readonly inkFaint: string;
  readonly success: string;
}

/** Returns the rows the picker occupies for one item count. */
export function pickerRows(itemCount: number): number {
  return paneSlotRows(itemCount + 1);
}

/** Renders the picker panel. */
export function Picker({
  items,
  selected,
  title,
  legend,
  marker,
  rowMarker,
  palette,
}: {
  items: readonly PickerItem[];
  selected: number;
  title: string;
  legend: string;
  /** The cursor marker and the row marker, already spelled. */
  marker: string;
  rowMarker: string;
  palette: PickerPalette;
}) {
  return (
    <Pane
      title={title}
      titleColor={palette.accentDeep}
      borderColor={palette.accent}
      background={palette.headerBg}
      paddingLeft={1}
      paddingRight={1}
      height={pickerRows(items.length)}
    >
      {items.map((item, index) => (
        <PickerRow
          key={item.key}
          item={item}
          highlighted={index === selected}
          marker={marker}
          rowMarker={rowMarker}
          palette={palette}
        />
      ))}
      <PickerLegend text={legend} palette={palette} />
    </Pane>
  );
}

/** One choice, cut to the columns the panel gives it. */
function PickerRow({
  item,
  highlighted,
  marker,
  rowMarker,
  palette,
}: {
  item: PickerItem;
  highlighted: boolean;
  marker: string;
  rowMarker: string;
  palette: PickerPalette;
}) {
  const columns = useContentColumns();
  const markerText = highlighted ? marker : rowMarker;
  const label = truncate(item.label, Math.max(0, columns - displayWidth(markerText)));
  const room = Math.max(0, columns - displayWidth(markerText) - displayWidth(label));
  const detail = `  ${item.detail}`;
  const current = item.current ? " current" : "";
  let detailText = detail;
  let currentText = "";
  if (displayWidth(detail) + displayWidth(current) <= room) {
    currentText = current;
  } else if (displayWidth(current) < room) {
    // The preference in force keeps its marker; the explanation gives the
    // columns it has left, and ends with an ellipsis where it was cut.
    detailText = truncate(detail, room - displayWidth(current));
    currentText = current;
  } else {
    detailText = truncate(detail, room);
  }
  return (
    <text wrapMode="none" bg={highlighted ? palette.rowSelectedBg : palette.headerBg}>
      <span fg={palette.accent} attributes={highlighted ? TextAttributes.BOLD : undefined}>
        {markerText}
      </span>
      <span fg={palette.ink} attributes={highlighted ? TextAttributes.BOLD : undefined}>
        {label}
      </span>
      <span fg={palette.inkMuted}>{detailText}</span>
      {currentText.length === 0 ? null : <span fg={palette.success}>{currentText}</span>}
    </text>
  );
}

/** The picker's one-line legend, cut to the panel's columns. */
function PickerLegend({ text, palette }: { text: string; palette: PickerPalette }) {
  const columns = useContentColumns();
  return (
    <text wrapMode="none" fg={palette.inkMuted} bg={palette.headerBg}>
      {truncate(text, columns)}
    </text>
  );
}
