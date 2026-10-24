//! The picker panel: one framed row per choice, with the cursor and the legend.
//!
//! The panel owns its frame, its row markers, its selected wash, and its legend;
//! a surface names the choices and the one the reader is on.

import { TextAttributes } from "@opentui/core";
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
      {items.map((item, index) => {
        const highlighted = index === selected;
        return (
          <text
            key={item.key}
            wrapMode="none"
            bg={highlighted ? palette.rowSelectedBg : palette.headerBg}
          >
            <span fg={palette.accent} attributes={highlighted ? TextAttributes.BOLD : undefined}>
              {highlighted ? marker : rowMarker}
            </span>
            <span fg={palette.ink} attributes={highlighted ? TextAttributes.BOLD : undefined}>
              {item.label}
            </span>
            <span fg={palette.inkMuted}>{`  ${item.detail}`}</span>
            {item.current ? <span fg={palette.success}> current</span> : null}
          </text>
        );
      })}
      <text wrapMode="none" fg={palette.inkFaint} bg={palette.headerBg}>
        {legend}
      </text>
    </Pane>
  );
}
