//! One chip of the input header.

/** Renders a label on its own wash, with one blank column on either side. */
export function Badge({
  label,
  ink,
  background,
}: {
  label: string;
  ink: string;
  background: string;
}) {
  return (
    <text bg={background} fg={ink}>
      {` ${label} `}
    </text>
  );
}
