//! One row of styled runs: a status line or a detail line.
//!
//! A run is text plus the ink it paints; the brick keys the runs, so a phrase
//! that repeats next to a separator still diffs on its own cell instead of on
//! an array position.

/** One styled run of a line. */
export interface Run {
  /** The text the run paints. */
  readonly text: string;
  /** The colour the run paints its text with. */
  readonly ink: string;
}

/** Renders the runs as one line that clips at the panel's width. */
export function Runs({ runs }: { runs: readonly Run[] }) {
  // A run's own ink and text name it: the same phrase can only repeat next to a
  // separator, so the pair is a key the row can be diffed on.
  const seen = new Map<string, number>();
  const keyed = runs.map((run) => {
    const base = `${run.ink}:${run.text}`;
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    return { key: count === 0 ? base : `${base}#${count}`, run };
  });
  return (
    <text wrapMode="none">
      {keyed.map(({ key, run }) => (
        <span key={key} fg={run.ink}>
          {run.text}
        </span>
      ))}
    </text>
  );
}
