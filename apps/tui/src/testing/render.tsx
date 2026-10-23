//! The rendering helper the view tests share.
//!
//! A view test renders one component at one panel size and reads the frame the
//! renderer painted. That is the whole contract: what a reader sees at that
//! size, in the cells the panel actually has.

import { testRender } from "@opentui/react/test-utils";
import type { ReactNode } from "react";

/** Renders one element at one panel size and returns the painted frame. */
export async function painted(element: ReactNode, width: number, height: number): Promise<string> {
  const { renderOnce, captureCharFrame, dispose } = await testRender(element, { width, height });
  await renderOnce();
  const frame = captureCharFrame();
  await dispose();
  return frame;
}

/** Returns a frame's lines, trimmed, with the trailing blank rows removed. */
export function frameLines(frame: string): string[] {
  const lines = frame.split("\n").map((line) => line.trim());
  while (lines.length > 0 && lines.at(-1) === "") {
    lines.pop();
  }
  return lines;
}
