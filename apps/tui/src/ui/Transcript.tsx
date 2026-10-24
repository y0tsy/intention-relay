//! The transcript: every committed row of the open session, and the step in
//! flight.
//!
//! A committed row becomes one block, and exactly one blank row separates two
//! consecutive blocks: the user card is padded from the reasoning above the
//! answer, the reasoning is padded from the answer, and an answer always starts
//! after a blank row. A tool exchange is one block drawn at the call's position,
//! so the result row it pairs with contributes no block of its own.
//!
//! The live tail is the step's two blocks before they commit - its reasoning,
//! then the answer it precedes - laid out by the very components the committed
//! row will use, so no live row looks provisional and none changes when the
//! commit arrives: the rows a step shows in the footer are the rows the printed
//! block paints.

import { selectExpansions, selectProvisionalReasoning, selectProvisionalText } from "../app/select";
import type { AppStore } from "../app/store";
import type { TranscriptBlock } from "../app/transcript";
import { Stack } from "../kit";
import type { Palette } from "../theme/palette";
import { Answer } from "./blocks/Answer";
import { Notice } from "./blocks/Notice";
import { Reasoning } from "./blocks/Reasoning";
import { ToolBlock } from "./blocks/ToolBlock";
import { UserCard } from "./blocks/UserCard";
import { useStore } from "./use-store";

/** The identity a live segment's expansion is kept under. */
export const LIVE_REASONING = "live";

/** Renders one committed block. */
export function CommittedBlock({
  block,
  palette,
  expansion,
}: {
  block: TranscriptBlock;
  palette: Palette;
  expansion: number;
}) {
  if (block.call !== null) {
    return <ToolBlock call={block.call} result={block.result} palette={palette} />;
  }
  const message = block.message;
  if (message === null) {
    return null;
  }
  switch (message.kind) {
    case "user":
      return <UserCard text={message.text} palette={palette} />;
    case "assistant":
      return (
        <Stack spaced>
          {message.reasoning === null || message.reasoning.trim().length === 0 ? null : (
            <Reasoning text={message.reasoning} palette={palette} expansion={expansion} />
          )}
          {message.text.trim().length === 0 ? null : (
            <Answer text={message.text} palette={palette} />
          )}
        </Stack>
      );
    case "notice":
      return <Notice text={message.text} palette={palette} />;
    case "tool_call":
    case "tool_result":
      return (
        <ToolBlock
          call={block.call}
          result={block.result ?? (message.kind === "tool_result" ? message : null)}
          palette={palette}
        />
      );
  }
}

/** Renders the step in flight: its reasoning, then the answer it precedes. */
export function Live({ store, palette }: { store: AppStore; palette: Palette }) {
  const expansions = useStore(store, selectExpansions);
  const reasoning = useStore(store, selectProvisionalReasoning);
  const text = useStore(store, selectProvisionalText);
  return (
    <Stack spaced>
      {reasoning.length === 0 ? null : (
        <Reasoning
          text={reasoning}
          palette={palette}
          expansion={expansions.get(LIVE_REASONING) ?? 0}
        />
      )}
      {text.length === 0 ? null : <Answer text={text} palette={palette} />}
    </Stack>
  );
}
