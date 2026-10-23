//! The status line: who this session is, and what is happening to the prompt.

import type { RunStatusDto } from "../bindings";
import type { Palette } from "../theme/palette";
import { describeRun, formatElapsed, shortId } from "./format";

/** The facts the status line renders. */
export interface StatusFacts {
  /** The open session's identity, or `null` before any session is open. */
  readonly sessionId: string | null;
  /** The run's lifecycle status, or `null` when the session reports none. */
  readonly runStatus: RunStatusDto | null;
  /** When the run started, in whole Unix seconds. */
  readonly startedAt: number | null;
  /** Whether the step in flight has produced answer text. */
  readonly answering: boolean;
  /** The committed tokens the run reports, or `null` when it reports none. */
  readonly tokens: number | null;
  /** The current time, in whole Unix seconds. */
  readonly now: number;
}

/** Returns the phrase that names what is happening to the prompt. */
export function statusPhrase(facts: StatusFacts): string {
  if (facts.runStatus === null) {
    return "ready";
  }
  return describeRun(facts.runStatus, facts.answering);
}

/** Returns the detail parts, in the order a reader needs them. */
export function statusParts(facts: StatusFacts): string[] {
  const parts: string[] = [];
  const elapsed = formatElapsed(facts.startedAt, facts.now);
  if (elapsed !== null) {
    parts.push(elapsed);
  }
  if (facts.tokens !== null) {
    parts.push(`${facts.tokens} tokens`);
  }
  return parts;
}

/** Returns the detail line's text: elapsed time and tokens, never a status. */
export function statusDetail(facts: StatusFacts): string {
  return statusParts(facts).join(" · ");
}

/** Renders the status line, dropping what the line cannot hold whole. */
export function StatusRow({
  facts,
  palette,
  width,
}: {
  facts: StatusFacts;
  palette: Palette;
  width: number;
}) {
  const phrase = statusPhrase(facts);
  const identity = facts.sessionId === null ? "no session" : shortId(facts.sessionId);
  // A part is kept whole or dropped whole, in the order a reader needs it: a
  // token count cut to "512 tok" tells a reader less than no count at all.
  let right = phrase;
  for (const part of statusParts(facts)) {
    const candidate = `${right} · ${part}`;
    if (candidate.length <= width - 2) {
      right = candidate;
    }
  }
  // The left half is the session's identity and the right half is the prompt's
  // fate: when the line cannot hold both, the fate is what a reader needs.
  const showIdentity = identity.length + right.length + 4 <= width;
  return (
    <box flexDirection="row" justifyContent="space-between" paddingLeft={1} paddingRight={1}>
      <text fg={palette.faint}>{showIdentity ? identity : ""}</text>
      <text fg={palette.muted}>{right}</text>
    </box>
  );
}
