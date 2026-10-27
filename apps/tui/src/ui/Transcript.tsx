//! The transcript: every committed row of the open session, and the step in
//! flight.
//!
//! A committed row becomes one block, and exactly one blank row separates two
//! consecutive blocks: the user card is padded from the reasoning above the
//! answer, the reasoning is padded from the answer, and an answer always starts
//! after a blank row. A tool exchange is one block drawn at the call's position,
//! so the result row it pairs with contributes no block of its own.
//!
//! The transcript is printed into the terminal's own scrollback, so these
//! components paint rows, not a region: the printer paints them into the page
//! once and the terminal keeps them. The step in flight paints the very
//! components the committed row uses, at the columns the printed rows have, so
//! the rows a step streams are the rows it keeps.

import { selectProvisionalReasoning, selectProvisionalText } from "../app/select";
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

/** Renders one committed block. */
export function CommittedBlock({ block, palette }: { block: TranscriptBlock; palette: Palette }) {
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
            <Reasoning text={message.reasoning} palette={palette} />
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
export function Live({
  reasoning,
  text,
  palette,
}: {
  /** The reasoning the step has streamed. */
  reasoning: string;
  /** The answer the step has streamed. */
  text: string;
  palette: Palette;
}) {
  return (
    <Stack spaced>
      {reasoning.length === 0 ? null : <Reasoning text={reasoning} palette={palette} />}
      {text.length === 0 ? null : <Answer text={text} palette={palette} />}
    </Stack>
  );
}

/** Paints the step in flight from the transcript's own transient text. */
export function LiveTail({ store, palette }: { store: AppStore; palette: Palette }) {
  const reasoning = useStore(store, selectProvisionalReasoning);
  const text = useStore(store, selectProvisionalText);
  return <Live reasoning={reasoning} text={text} palette={palette} />;
}
