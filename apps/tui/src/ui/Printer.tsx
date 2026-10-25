//! The printer's transcript: the blocks a session has committed, and the step in
//! flight.
//!
//! The kernel owns the surface, the sealing, and the chunking; this module names
//! what a print carries and paints it. A run that has finished contributes one
//! more block, the line the transcript closes the turn with, built from the run
//! projection the store carries.
//!
//! The step in flight prints its rows as it streams, so the answer block it
//! becomes must not be printed a second time: the adapter remembers what it
//! streamed and adopts the committed block that carries it. That bookkeeping is
//! decided while rendering, before the printer takes the list, so no block is
//! adopted after the printer has already read it.

import { useMemo, useRef } from "react";
import {
  selectActiveRun,
  selectProvisionalReasoning,
  selectProvisionalText,
  selectSessionId,
  selectTranscript,
} from "../app/select";
import type { AppStore } from "../app/store";
import { type PrintBlock, transcriptBlocks, withRunMeta } from "../app/transcript";
import { Printer as KernelPrinter, type StepPrint } from "../kit";
import type { Palette } from "../theme/palette";
import { RunMeta } from "./blocks/RunMeta";
import { CommittedBlock, Live } from "./Transcript";
import { useStore } from "./use-store";

/** The text a step streamed before it committed. */
export interface Streamed {
  readonly reasoning: string;
  readonly text: string;
}

/** Nothing adopted yet. */
const NOTHING: ReadonlySet<string> = new Set();

/**
 * Returns the key of the committed answer a streamed step became.
 *
 * The step in flight prints its rows while it streams, so the block the daemon
 * commits for that step is already in the terminal. The stream and the block
 * carry the same text and are painted by the same components, so the text is
 * what tells the printer which block it has.
 */
export function adoptedAnswer(
  entries: readonly PrintBlock[],
  adopted: ReadonlySet<string>,
  streamed: Streamed,
): string | null {
  for (const entry of entries) {
    if ("run" in entry || adopted.has(entry.key)) {
      continue;
    }
    const message = entry.message;
    if (message === null || message.kind !== "assistant") {
      continue;
    }
    if (message.text.trimEnd() !== streamed.text.trimEnd()) {
      continue;
    }
    if ((message.reasoning ?? "").trimEnd() !== streamed.reasoning.trimEnd()) {
      continue;
    }
    return entry.key;
  }
  return null;
}

/** Prints the committed blocks the terminal does not have yet, and the step in flight. */
export function Printer({
  store,
  palette,
  onPrinted,
}: {
  store: AppStore;
  palette: Palette;
  /** Called with the rows a print takes, so the footer gives them back first. */
  onPrinted: (rows: number) => void;
}) {
  const session = useStore(store, selectSessionId);
  const messages = useStore(store, selectTranscript);
  const run = useStore(store, selectActiveRun);
  const reasoning = useStore(store, selectProvisionalReasoning);
  const text = useStore(store, selectProvisionalText);
  const entries = useMemo(() => withRunMeta(transcriptBlocks(messages), run), [messages, run]);
  // What this session's page already carries: the answer blocks a streamed step
  // became, and the text of the step streaming now.
  const page = useRef<{
    session: string | null;
    adopted: ReadonlySet<string>;
    streamed: Streamed | null;
  }>({ session, adopted: NOTHING, streamed: null });
  if (page.current.session !== session) {
    page.current = { session, adopted: NOTHING, streamed: null };
  }
  const live = text.length > 0 || reasoning.length > 0;
  if (live) {
    page.current.streamed = { reasoning, text };
  } else if (page.current.streamed !== null) {
    const key = adoptedAnswer(entries, page.current.adopted, page.current.streamed);
    if (key !== null) {
      page.current.adopted = new Set([...page.current.adopted, key]);
      page.current.streamed = null;
    }
  }
  const adopted = page.current.adopted;
  const blocks = adopted.size === 0 ? entries : entries.filter((entry) => !adopted.has(entry.key));
  const step = useMemo<StepPrint | null>(
    () => (live ? { reasoning, text } : null),
    [live, reasoning, text],
  );
  return (
    <KernelPrinter
      session={session}
      blocks={blocks}
      step={step}
      palette={palette}
      onPrinted={onPrinted}
      renderBlock={(block) =>
        "run" in block ? (
          <RunMeta run={block.run} palette={palette} />
        ) : (
          <CommittedBlock block={block} palette={palette} />
        )
      }
      renderStream={(snapshot) => (
        <Live reasoning={snapshot.reasoning} text={snapshot.text} palette={palette} />
      )}
    />
  );
}
