//! A hero panel: the lockup over the overview it was given.
//!
//! The overview is pinned to the panel's last rows and the lockup sits at its
//! top, the filler between them. A surface too short for both keeps the overview,
//! and a surface with no room draws nothing.

import type { ReactNode } from "react";
import { SlotRows } from "./context";
import { HERO_ROWS, heroLayout } from "./hero-plan";
import { Stack } from "./Stack";

/** Renders the lockup over the overview pinned to the last rows. */
export function Hero({
  rows,
  lockup,
  overview,
  reserve = 3,
  lockupRows = HERO_ROWS,
}: {
  rows: number;
  lockup: ReactNode;
  /** Renders the overview in the rows the pane pins to its bottom. */
  overview: (rows: number) => ReactNode;
  /** The rows the overview may keep before the lockup takes the rest. */
  reserve?: number;
  /** The rows the lockup itself occupies. */
  lockupRows?: number;
}) {
  if (rows < 1) {
    return null;
  }
  const plan = heroLayout(rows, reserve, lockupRows);
  return (
    <Stack height={rows}>
      {plan.lead > 0 ? <box height={plan.lead} /> : null}
      {plan.hero > 0 ? lockup : null}
      {plan.filler > 0 ? <box height={plan.filler} /> : null}
      {plan.overview > 0 ? (
        <SlotRows rows={plan.overview}>{overview(plan.overview)}</SlotRows>
      ) : null}
    </Stack>
  );
}
