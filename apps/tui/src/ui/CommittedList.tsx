//! The committed rows of the open session.
//!
//! The list subscribes to the rows itself, so a frame that only moves the
//! transient tail never reaches it, and every row is memoized on its own
//! identity: an appended row lays out the appended row alone, exactly as a
//! direct layout would.

import { TextAttributes } from "@opentui/core";
import { memo } from "react";
import { selectTranscript } from "../app/select";
import type { AppStore } from "../app/store";
import type { MessageProjectionDto, ToolRowDto } from "../bindings";
import type { Palette } from "../theme/palette";
import { Markdown } from "./Markdown";
import { useStore } from "./use-store";

/**
 * Returns the one-line label of a tool row.
 *
 * The identity travels as the wire name the model used, so a row names the tool
 * exactly as it was called: there is no declared-tool vocabulary to look up and
 * nothing to fall back to when the name is one the product does not expose.
 */
export function toolLabel(tool: ToolRowDto): string {
  switch (tool.row) {
    case "call":
      return tool.value.tool_id;
    case "result":
      return "result";
  }
}

/** Renders one committed row. */
const Row = memo(function Row({
  message,
  palette,
}: {
  message: MessageProjectionDto;
  palette: Palette;
}) {
  switch (message.kind) {
    case "user":
      return (
        <text wrapMode="word" fg={palette.ink}>
          <span attributes={TextAttributes.BOLD}>{"> "}</span>
          {message.text}
        </text>
      );
    case "assistant":
      return <Markdown source={message.text} palette={palette} />;
    case "tool_call":
    case "tool_result":
      return (
        <text wrapMode="word" fg={palette.toolPreview}>
          {message.tool === null
            ? message.text
            : `${toolLabel(message.tool)} ${message.text}`.trimEnd()}
        </text>
      );
    case "notice":
      return (
        <text wrapMode="word" fg={palette.inkMuted}>
          {message.text}
        </text>
      );
  }
});

/** Renders every committed row, newest last. */
export function CommittedList({ store, palette }: { store: AppStore; palette: Palette }) {
  const messages = useStore(store, selectTranscript);
  return (
    <>
      {messages.map((message) => (
        <Row key={message.id} message={message} palette={palette} />
      ))}
    </>
  );
}
