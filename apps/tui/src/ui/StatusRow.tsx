//! The status line: who this session is, and what is happening to the prompt.

import {
  selectAnswering,
  selectLive,
  selectRunStatus,
  selectSessionId,
  selectStartedAt,
  selectTokens,
} from "../app/select";
import type { AppStore } from "../app/store";
import type { RunStatusDto } from "../bindings";
import type { Palette } from "../theme/palette";
import { describeRun, formatElapsed, shortId } from "./format";
import { useNow } from "./use-now";
import { useStore } from "./use-store";

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
      <text fg={palette.inkFaint}>{showIdentity ? identity : ""}</text>
      <text fg={palette.inkMuted}>{right}</text>
    </box>
  );
}

/**
 * Renders the status line from the store.
 *
 * The line subscribes to the facts it prints and keeps the clock itself, so a
 * token count that moves with the stream, and the second that passes, repaint
 * this one row.
 */
export function StatusLine({
  store,
  palette,
  width,
}: {
  store: AppStore;
  palette: Palette;
  width: number;
}) {
  const sessionId = useStore(store, selectSessionId);
  const runStatus = useStore(store, selectRunStatus);
  const startedAt = useStore(store, selectStartedAt);
  const answering = useStore(store, selectAnswering);
  const tokens = useStore(store, selectTokens);
  const live = useStore(store, selectLive);
  // The elapsed time only moves while a run is live, so the clock runs then and
  // is silent otherwise.
  const now = useNow(live);
  const facts: StatusFacts = { sessionId, runStatus, startedAt, answering, tokens, now };
  return <StatusRow facts={facts} palette={palette} width={width} />;
}
