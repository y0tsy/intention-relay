//! The welcome block's composition for one panel size.
//!
//! A terminal panel has a fixed number of rows, and a block that asks for more
//! than it has gets squashed: rows overlap and text runs into text. So the block
//! is planned, not styled: the richest composition that fits the rows it is
//! given is the one that is rendered, and every composition is built from whole
//! lines.

import { COMMANDS, typedName } from "../app/commands";
import { fitExamples } from "./format";

/** The examples the welcome block offers, richest first. */
export const EXAMPLES: readonly string[] = [
  "explain this repository",
  "find the test that covers this",
  "add a regression test",
  "summarize what changed today",
];

/** The width prose is wrapped at, so a line stays readable on a wide terminal. */
export const PROSE_WIDTH = 78;

/** The longest tagline, shown where the panel can hold it. */
const LONG_TAGLINE =
  "One session, one transcript. Type a prompt and press enter; the reply streams into this panel while the run is live.";

/** The tagline shown where the panel cannot. */
const SHORT_TAGLINE = "Type a prompt and press enter.";

/** How the command reference is shown, if at all. */
export type CommandReference = "list" | "hint" | "none";

/** One welcome composition. */
export interface WelcomePlan {
  /** Whether the rule under the lockup is drawn. */
  readonly rule: boolean;
  /** The tagline to show, or `null` when the panel holds none. */
  readonly tagline: string | null;
  /** The examples that fit, in order. */
  readonly examples: readonly string[];
  /** How the command reference is shown. */
  readonly commands: CommandReference;
  /** Whether the workspace line is shown at the bottom. */
  readonly workspace: boolean;
}

/** The panel one welcome block is planned for. */
export interface WelcomeRoom {
  /** The columns the block may use. */
  readonly width: number;
  /** The rows the block may use. */
  readonly height: number;
  /** The workspace root the block reports. */
  readonly workspace: string;
}

/** Renders one line of text at a width, counting the rows it wraps onto. */
function rowsFor(text: string, width: number): number {
  if (width <= 0) {
    return 1;
  }
  return Math.max(1, Math.ceil(text.length / width));
}

/** Returns the rows one composition needs. */
export function planRows(plan: WelcomePlan, width: number, workspaceLine: string): number {
  const blocks: number[] = [1];
  if (plan.rule) {
    blocks.push(1);
  }
  if (plan.tagline !== null) {
    blocks.push(rowsFor(plan.tagline, width));
  }
  if (plan.examples.length > 0) {
    blocks.push(plan.examples.length);
  }
  if (plan.commands === "list") {
    blocks.push(COMMANDS.length);
  } else if (plan.commands === "hint") {
    blocks.push(1);
  }
  if (plan.workspace) {
    blocks.push(rowsFor(workspaceLine, width));
  }
  // One blank row between blocks, which is what the block's own gap costs.
  return blocks.reduce((total, rows) => total + rows, 0) + Math.max(0, blocks.length - 1);
}

/**
 * Returns the richest composition that fits `room`.
 *
 * Every composition that fits is scored, and the best one wins: examples count
 * for most, then the command reference, then the tagline, then the rule. A part
 * is dropped whole, never shortened, and the lockup is never dropped.
 */
export function planWelcome(room: WelcomeRoom): WelcomePlan {
  const textWidth = Math.max(8, Math.min(room.width - 4, PROSE_WIDTH));
  const workspaceLine = `new sessions run in ${room.workspace}`;
  const examples = fitExamples(textWidth - 2, EXAMPLES);
  const references: CommandReference[] = ["list", "hint", "none"];
  const taglines: Array<string | null> = [LONG_TAGLINE, SHORT_TAGLINE, null];

  let best: WelcomePlan | null = null;
  let bestScore = -1;
  for (const tagline of taglines) {
    for (let kept = examples.length; kept >= 0; kept -= 1) {
      for (const commands of references) {
        for (const workspace of [true, false]) {
          for (const rule of [true, false]) {
            const plan: WelcomePlan = {
              rule,
              tagline,
              examples: examples.slice(0, kept),
              commands,
              workspace,
            };
            if (planRows(plan, textWidth, workspaceLine) > room.height) {
              continue;
            }
            const score =
              kept * 3 +
              (commands === "list" ? 4 : commands === "hint" ? 2 : 0) +
              (tagline === LONG_TAGLINE ? 2 : tagline === SHORT_TAGLINE ? 1 : 0) +
              (workspace ? 2 : 0) +
              (rule ? 1 : 0);
            if (score > bestScore) {
              bestScore = score;
              best = plan;
            }
          }
        }
      }
    }
  }
  return best ?? { rule: false, tagline: null, examples: [], commands: "none", workspace: false };
}

/** Returns the text of one command reference row, or the name alone when it will not fit. */
export function commandRow(
  command: { readonly name: string; readonly summary: string },
  width: number,
): string {
  const typed = typedName(command);
  return typed.length + 2 + command.summary.length > width ? typed : `${typed}  ${command.summary}`;
}
