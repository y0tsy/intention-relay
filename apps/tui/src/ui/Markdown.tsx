//! Markdown as the transcript renders it.
//!
//! `marked` tokenizes and this module decides what each token looks like: the
//! app owns the indentation, the palette, and the wrap, which a ready-made
//! renderer would take over. Only the block types a chat answer actually uses
//! are handled, and anything else renders as its own text.

import { TextAttributes } from "@opentui/core";
import type { Token, Tokens } from "marked";
import { marked } from "marked";
import type { ReactNode } from "react";
import type { Palette } from "../theme/palette";

/** One inline run of text and how it is marked. */
interface Run {
  readonly text: string;
  readonly strong?: boolean;
  readonly emphasis?: boolean;
  readonly code?: boolean;
  readonly link?: boolean;
}

/** Returns the text of one inline token tree, flattened for a plain line. */
function flatten(token: Token): Run[] {
  switch (token.type) {
    case "text":
    case "escape": {
      const text = token as Tokens.Text;
      if ("tokens" in text && text.tokens !== undefined) {
        return text.tokens.flatMap(flatten);
      }
      return [{ text: text.text }];
    }
    case "strong":
      return (token as Tokens.Strong).tokens.flatMap((inner) =>
        flatten(inner).map((run) => ({ ...run, strong: true })),
      );
    case "em":
      return (token as Tokens.Em).tokens.flatMap((inner) =>
        flatten(inner).map((run) => ({ ...run, emphasis: true })),
      );
    case "codespan":
      return [{ text: (token as Tokens.Codespan).text, code: true }];
    case "link":
      return (token as Tokens.Link).tokens.flatMap((inner) =>
        flatten(inner).map((run) => ({ ...run, link: true })),
      );
    case "br":
      return [{ text: "\n" }];
    case "html":
      return [];
    default:
      return "raw" in token ? [{ text: String(token.raw) }] : [];
  }
}

/** Renders one run with the palette's mark for what it is. */
function runElement(run: Run, palette: Palette, index: number): ReactNode {
  const color = run.code ? palette.code : run.link ? palette.link : undefined;
  const attributes = run.strong
    ? TextAttributes.BOLD
    : run.emphasis
      ? TextAttributes.ITALIC
      : undefined;
  const content = run.code ? ` ${run.text} ` : run.text;
  return (
    <span key={index} fg={color} attributes={attributes}>
      {content}
    </span>
  );
}

/** Renders one inline token tree as spans. */
function inline(tokens: readonly Token[], palette: Palette): ReactNode[] {
  return tokens.flatMap(flatten).map((run, index) => runElement(run, palette, index));
}

/** Returns the number a list item starts at, when the list declares one. */
function listStart(list: Tokens.List): number {
  return typeof list.start === "number" && list.start > 0 ? list.start : 1;
}

/** Returns a stable identity for one markdown block. */
function blockKey(index: number, raw: string): string {
  return `${index}:${raw.slice(0, 32)}`;
}

/** Renders one markdown block. */
function block(token: Token, palette: Palette, key: string): ReactNode {
  switch (token.type) {
    case "paragraph": {
      const paragraph = token as Tokens.Paragraph;
      return (
        <text key={key} wrapMode="word" fg={palette.assistant}>
          {inline(paragraph.tokens, palette)}
        </text>
      );
    }
    case "heading": {
      const heading = token as Tokens.Heading;
      return (
        <text key={key} wrapMode="word" fg={palette.foreground} attributes={TextAttributes.BOLD}>
          {inline(heading.tokens, palette)}
        </text>
      );
    }
    case "list": {
      const list = token as Tokens.List;
      const start = listStart(list);
      return (
        <box key={key} flexDirection="column">
          {list.items.map((item, index) => (
            <box key={blockKey(index, item.raw)} flexDirection="row">
              <text fg={palette.muted}>{list.ordered ? `${start + index}. ` : "- "}</text>
              <box flexDirection="column" flexGrow={1}>
                {item.tokens.map((inner, innerIndex) =>
                  block(inner, palette, blockKey(innerIndex, inner.raw)),
                )}
              </box>
            </box>
          ))}
        </box>
      );
    }
    case "code": {
      const code = token as Tokens.Code;
      return (
        <box
          key={key}
          flexDirection="column"
          paddingLeft={1}
          borderStyle="single"
          border={["left"]}
          borderColor={palette.border}
        >
          <text wrapMode="word" fg={palette.code}>
            {code.text}
          </text>
        </box>
      );
    }
    case "blockquote": {
      const quote = token as Tokens.Blockquote;
      return (
        <box
          key={key}
          flexDirection="column"
          paddingLeft={1}
          borderStyle="single"
          border={["left"]}
          borderColor={palette.border}
        >
          {quote.tokens.map((inner, innerIndex) =>
            block(inner, palette, blockKey(innerIndex, inner.raw)),
          )}
        </box>
      );
    }
    case "space":
      return <box key={key} height={1} />;
    case "hr":
      return (
        <text key={key} fg={palette.faint}>
          {"─".repeat(3)}
        </text>
      );
    case "html":
      return null;
    default: {
      const raw = "raw" in token ? token.raw : "";
      return (
        <text key={key} wrapMode="word" fg={palette.assistant}>
          {raw}
        </text>
      );
    }
  }
}

/**
 * Renders one markdown document.
 *
 * Blocks are laid out one after another, so a paragraph wraps by word and a
 * fenced block keeps its own lines: the two decisions a terminal reader notices
 * first.
 */
export function Markdown({ source, palette }: { source: string; palette: Palette }) {
  const tokens = marked.lexer(source);
  return (
    <box flexDirection="column">
      {tokens.map((token, index) => block(token, palette, blockKey(index, token.raw)))}
    </box>
  );
}
