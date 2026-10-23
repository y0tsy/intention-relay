//! One display width for every surface.
//!
//! A terminal measures text in columns, and a wide character takes two. The stack
//! carries the measurement its own edit buffer uses, so every wrap point, clip,
//! and centred run reads the same number the renderer will paint.

import { measureEditText } from "@opentui/core";

/** Returns the display columns a string occupies. */
export function displayWidth(text: string): number {
  return measureEditText(text, { widthMethod: "unicode" });
}
