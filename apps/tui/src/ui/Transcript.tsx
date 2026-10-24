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

import {
  selectExpansions,
  selectProvisionalReasoning,
  selectProvisionalText,
  selectTranscript,
} from "../app/select";
import type { AppStore } from "../app/store";
import type { MessageProjectionDto } from "../bindings";
import { ContentColumns } from "../kit";
import type { Palette } from "../theme/palette";
import { Answer } from "./blocks/Answer";
import { Notice } from "./blocks/Notice";
import { Reasoning } from "./blocks/Reasoning";
import { ToolBlock } from "./blocks/ToolBlock";
import { UserCard } from "./blocks/UserCard";
import { useStore } from "./use-store";

/** The identity a live segment's expansion is kept under. */
export const LIVE_REASONING = "live";

/** One block of the transcript: the rows it draws and what decides their state. */
export interface Block {
  readonly key: string;
  readonly message: MessageProjectionDto | null;
  readonly call: MessageProjectionDto | null;
  readonly result: MessageProjectionDto | null;
}

/** Returns the call identity one tool row carries. */
function callId(message: MessageProjectionDto): string | null {
  const tool = message.tool;
  return tool === null ? null : tool.value.call_id;
}

/**
 * Pairs every tool call with the result that answers it.
 *
 * A result whose call is committed is drawn by that call and returns `null`
 * here, so an exchange is shown exactly once; a result without its call stays
 * a block of its own and says so.
 */
export function blocks(messages: readonly MessageProjectionDto[]): Block[] {
  const results = new Map<string, MessageProjectionDto>();
  for (const message of messages) {
    if (message.kind === "tool_result") {
      const id = callId(message);
      if (id !== null) {
        results.set(id, message);
      }
    }
  }
  const drawn = new Set(results.values());
  return messages
    .filter((message) => !drawn.has(message))
    .map((message) => {
      const id = message.kind === "tool_call" ? callId(message) : null;
      const result = id === null ? null : (results.get(id) ?? null);
      const key = String(message.id);
      return id === null
        ? { key, message, call: null, result: null }
        : { key, message, call: message, result };
    });
}

/** Renders one committed block. */
export function CommittedBlock({
  block,
  palette,
  expansion,
}: {
  block: Block;
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
        <>
          {message.reasoning === null || message.reasoning.trim().length === 0 ? null : (
            <Reasoning text={message.reasoning} palette={palette} expansion={expansion} />
          )}
          {message.text.trim().length === 0 ? null : (
            <Answer text={message.text} palette={palette} />
          )}
        </>
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

/** Renders the committed rows, one block per message, a blank row between. */
export function Committed({
  store,
  palette,
  width,
}: {
  store: AppStore;
  palette: Palette;
  /** The column the blocks read; the parent's content width when omitted. */
  width?: number;
}) {
  const messages = useStore(store, selectTranscript);
  const expansions = useStore(store, selectExpansions);
  const column = (
    <box flexDirection="column" gap={1}>
      {blocks(messages).map((block) => (
        <CommittedBlock
          key={block.key}
          block={block}
          palette={palette}
          expansion={expansions.get(block.key) ?? 0}
        />
      ))}
    </box>
  );
  return width === undefined ? column : <ContentColumns columns={width}>{column}</ContentColumns>;
}

/** Renders the step in flight: its reasoning, then the answer it precedes. */
export function Live({
  store,
  palette,
  width,
}: {
  store: AppStore;
  palette: Palette;
  /** The column the blocks read; the parent's content width when omitted. */
  width?: number;
}) {
  const expansions = useStore(store, selectExpansions);
  const reasoning = useStore(store, selectProvisionalReasoning);
  const text = useStore(store, selectProvisionalText);
  const column = (
    <box flexDirection="column" gap={1}>
      {reasoning.length === 0 ? null : (
        <Reasoning
          text={reasoning}
          palette={palette}
          expansion={expansions.get(LIVE_REASONING) ?? 0}
        />
      )}
      {text.length === 0 ? null : <Answer text={text} palette={palette} />}
    </box>
  );
  return width === undefined ? column : <ContentColumns columns={width}>{column}</ContentColumns>;
}
