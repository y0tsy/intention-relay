//! The paste bytes a renderer reports, decoded once at the surface boundary.

import { decodePasteBytes } from "@opentui/core";

/**
 * Returns the text one paste carries.
 *
 * A paste is data, never a gesture: its newlines are the prompt's own, so a
 * pasted block lands in the draft whole instead of submitting at the first line
 * break the way a typed return would.
 */
export function pastedText(bytes: Uint8Array): string {
  return decodePasteBytes(bytes);
}
