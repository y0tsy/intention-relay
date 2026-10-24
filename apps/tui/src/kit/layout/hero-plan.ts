//! How a hero panel spends its rows, top to bottom.
//!
//! The overview the panel shows is pinned to its last rows, directly above the
//! input; the lockup sits at the top and the filler keeps them apart. A panel
//! too short for the lockup keeps the overview first, and a panel with room
//! for nothing spends nothing.

/** The rows above the hero panel's lockup: margin, frame, padding. */
export const HERO_LEAD_ROWS = 1;

/** The rows the lockup itself occupies. */
export const HERO_ROWS = 1;

/** How `Hero` spends the rows it was given. */
export interface HeroPlan {
  /** The blank rows above the lockup. */
  readonly lead: number;
  /** The rows the lockup occupies. */
  readonly hero: number;
  /** The blank rows between the lockup and the overview. */
  readonly filler: number;
  /** The overview's rows, pinned to the panel's last rows. */
  readonly overview: number;
}

/** Returns how `rows` rows carry the lockup over the pinned overview. */
export function heroLayout(rows: number, reserve = 3): HeroPlan {
  const overview = Math.min(rows, reserve);
  const above = rows - overview;
  const hero = Math.min(above, HERO_LEAD_ROWS + HERO_ROWS);
  return {
    lead: hero >= HERO_LEAD_ROWS + HERO_ROWS ? HERO_LEAD_ROWS : 0,
    hero: hero > 0 ? hero - (hero >= HERO_LEAD_ROWS + HERO_ROWS ? HERO_LEAD_ROWS : 0) : 0,
    filler: above - hero,
    overview,
  };
}
