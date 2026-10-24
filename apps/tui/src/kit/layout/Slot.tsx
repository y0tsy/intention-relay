//! One footer slot: the rows the plan gave this region.

import type { ReactNode } from "react";
import { useSlotRows } from "./context";
import { Stack } from "./Stack";

/** Renders children in the slot's rows, and nothing in an empty slot. */
export function Slot({ children, background }: { children?: ReactNode; background?: string }) {
  const rows = useSlotRows(0);
  if (rows < 1) {
    return null;
  }
  return (
    <Stack height={rows} background={background}>
      {children}
    </Stack>
  );
}
