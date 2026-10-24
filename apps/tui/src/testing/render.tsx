//! The rendering helper the view tests share.
//!
//! A view test renders one component at one panel size and reads the frame the
//! renderer painted. That is the whole contract: what a reader sees at that
//! size, in the cells the panel actually has.
//!
//! The application itself is a footer over a terminal that owns the transcript,
//! so its tests read two things: the rows the window paints, and the rows it
//! printed above them.

import type { TestRendererSetup } from "@opentui/core/testing";
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

/**
 * Renders the application in a window `height` rows tall and returns its setup.
 *
 * The application paints a footer over a terminal that keeps the transcript, so
 * the renderer is put in the mode the running app uses: the frame it captures is
 * the footer, and the rows it commits are the ones the terminal would scroll.
 */
export async function mountApp(
  element: ReactNode,
  width: number,
  height: number,
): Promise<TestRendererSetup> {
  const setup = await testRender(element, {
    width,
    height,
    screenMode: "split-footer",
    footerHeight: height,
    externalOutputMode: "capture-stdout",
  });
  // The footer tells the renderer how many rows it takes, so the first frames
  // settle the region before a test reads a cell.
  await setup.renderOnce();
  await setup.flush();
  await setup.renderOnce();
  return setup;
}

/** Returns the rows the application printed into the terminal, and forgets them. */
export function printedRows(setup: TestRendererSetup): string[] {
  return setup.externalOutput.take().flatMap((commit) => commit.rows);
}

/** Returns a frame's lines, trimmed, with the trailing blank rows removed. */
export function frameLines(frame: string): string[] {
  const lines = frame.split("\n").map((line) => line.trim());
  while (lines.length > 0 && lines.at(-1) === "") {
    lines.pop();
  }
  return lines;
}

/** Returns one setup's painted frame as trimmed lines. */
export function paintedLines(setup: TestRendererSetup): string[] {
  return frameLines(setup.captureCharFrame());
}
