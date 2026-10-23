//! The transcript: committed rows, then the text of the step in flight.

import { TextAttributes } from "@opentui/core";
import type { MessageProjectionDto, ToolIdentityDto, ToolRowDto } from "../bindings";
import type { Palette } from "../theme/palette";
import { Markdown } from "./Markdown";

/** Returns the name a tool row reports. */
export function toolName(identity: ToolIdentityDto): string {
  return "Declared" in identity ? identity.Declared : identity.Undeclared;
}

/** Returns the one-line label of a tool row. */
export function toolLabel(tool: ToolRowDto): string {
  switch (tool.row) {
    case "call":
      return toolName(tool.value.tool_id);
    case "result":
      return "result";
  }
}

/** Renders one committed row. */
function Row({ message, palette }: { message: MessageProjectionDto; palette: Palette }) {
  switch (message.kind) {
    case "user":
      return (
        <text wrapMode="word" fg={palette.user}>
          <span attributes={TextAttributes.BOLD}>{"> "}</span>
          {message.text}
        </text>
      );
    case "assistant":
      return <Markdown source={message.text} palette={palette} />;
    case "tool_call":
    case "tool_result":
      return (
        <text wrapMode="word" fg={palette.tool}>
          {message.tool === null
            ? message.text
            : `${toolLabel(message.tool)} ${message.text}`.trimEnd()}
        </text>
      );
    case "notice":
      return (
        <text wrapMode="word" fg={palette.muted}>
          {message.text}
        </text>
      );
  }
}

/** Renders every committed row and the transient text that follows them. */
export function Transcript({
  messages,
  provisionalReasoning,
  provisionalText,
  palette,
}: {
  messages: readonly MessageProjectionDto[];
  provisionalReasoning: string;
  provisionalText: string;
  palette: Palette;
}) {
  return (
    <box flexDirection="column" gap={1}>
      {messages.map((message) => (
        <Row key={message.id} message={message} palette={palette} />
      ))}
      {provisionalReasoning.length > 0 ? (
        <text wrapMode="word" fg={palette.faint}>
          {provisionalReasoning}
        </text>
      ) : null}
      {provisionalText.length > 0 ? (
        <text wrapMode="word" fg={palette.provisional}>
          {provisionalText}
        </text>
      ) : null}
    </box>
  );
}
