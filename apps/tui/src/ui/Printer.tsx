//! The printer's transcript: the blocks a session has committed, in order.
//!
//! The kernel owns the surface, the folding, and the chunking; this module only
//! names the block a key stands for and renders it with the expansion the store
//! keeps for it. A run that has finished contributes one more block, the line the
//! transcript closes the turn with, built from the run projection the store
//! carries.

import { useMemo } from "react";
import {
  selectActiveRun,
  selectExpansions,
  selectSessionId,
  selectTranscript,
} from "../app/select";
import type { AppStore } from "../app/store";
import { transcriptBlocks, withRunMeta } from "../app/transcript";
import { Printer as KernelPrinter } from "../kit";
import type { Palette } from "../theme/palette";
import { RunMeta } from "./blocks/RunMeta";
import { CommittedBlock } from "./Transcript";
import { useStore } from "./use-store";

/** Prints the committed blocks the terminal does not have yet. */
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
  const expansions = useStore(store, selectExpansions);
  const run = useStore(store, selectActiveRun);
  const blocks = useMemo(() => withRunMeta(transcriptBlocks(messages), run), [messages, run]);
  return (
    <KernelPrinter
      session={session}
      blocks={blocks}
      palette={palette}
      onPrinted={onPrinted}
      renderBlock={(block) =>
        "run" in block ? (
          <RunMeta run={block.run} palette={palette} />
        ) : (
          <CommittedBlock
            block={block}
            palette={palette}
            expansion={expansions.get(block.key) ?? 0}
          />
        )
      }
    />
  );
}
