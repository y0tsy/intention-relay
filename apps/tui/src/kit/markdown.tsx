//! Markdown as the transcript renders it.
//!
//! `marked` tokenizes and this module decides what each token looks like: the
//! app owns the indentation, the palette, and the wrap, which a ready-made
//! renderer would take over. Only the block types a chat answer actually uses
//! are handled, and anything else renders as its own text.
//!
//! Parsing is split from painting so a streamed answer is not re-parsed from its
//! first word on every frame: [`MarkdownBlock`] parses the source it is handed
//! once, and [`MarkdownBlocks`] hands it the settled prefix and the open tail as
//! two sources.

import { TextAttributes } from "@opentui/core";
import type { Token, Tokens } from "marked";
import { marked } from "marked";
import { memo, type ReactNode, useMemo } from "react";
import { useStaticColumns } from "./layout/context";
import { Pane } from "./layout/Pane";
import { Rule } from "./layout/Rule";
import { Table, type TableColumn } from "./layout/Table";
import { splitBlocks } from "./markdown-split";
import { displayWidth } from "./text";

/** The palette roles a markdown block paints. */
export interface MarkdownPalette {
  readonly ink: string;
  readonly inkMuted: string;
  readonly inkFaint: string;
  readonly border: string;
  readonly codeSurface: string;
  readonly markdownHeading: string;
  readonly markdownCode: string;
  /** The page's accent, which a list marker wears. */
  readonly accent: string;

  readonly markdownLink: string;
}

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
function runElement(run: Run, palette: MarkdownPalette, index: number): ReactNode {
  const color = run.code ? palette.markdownCode : run.link ? palette.markdownLink : undefined;
  const attributes = run.strong
    ? TextAttributes.BOLD
    : run.emphasis
      ? TextAttributes.ITALIC
      : undefined;
  // Inline code carries its own mark, never padding: the source's punctuation
  // stays against the word it belongs to.
  const content = run.text;
  return (
    <span key={index} fg={color} attributes={attributes}>
      {content}
    </span>
  );
}

/** Renders one inline token tree as spans. */
function inline(tokens: readonly Token[], palette: MarkdownPalette): ReactNode[] {
  return tokens.flatMap(flatten).map((run, index) => runElement(run, palette, index));
}

/** Returns the number a list item starts at, when the list declares one. */
function listStart(list: Tokens.List): number {
  return typeof list.start === "number" && list.start > 0 ? list.start : 1;
}

/**
 * Returns one markdown table row as the cells the table brick reads.
 *
 * The columns are keyed by position: a markdown table has no names of its own,
 * and the same key has to reach the header row and every body row.
 */
function tableCells(cells: readonly Tokens.TableCell[]): Record<string, string> {
  return Object.fromEntries(cells.map((cell, index) => [`c${index}`, cell.text]));
}

/** Returns a stable identity for one markdown block. */
function blockKey(index: number, raw: string): string {
  return `${index}:${raw.slice(0, 32)}`;
}

/** Renders one markdown block at the columns its page has. */
function block(
  token: Token,
  palette: MarkdownPalette,
  ink: string,
  key: string,
  columns: number,
): ReactNode {
  switch (token.type) {
    case "paragraph": {
      const paragraph = token as Tokens.Paragraph;
      return (
        <text key={key} wrapMode="word" fg={ink}>
          {inline(paragraph.tokens, palette)}
        </text>
      );
    }
    case "heading": {
      const heading = token as Tokens.Heading;
      return (
        <text
          key={key}
          wrapMode="word"
          fg={palette.markdownHeading}
          attributes={TextAttributes.BOLD}
        >
          {inline(heading.tokens, palette)}
        </text>
      );
    }
    case "list": {
      const list = token as Tokens.List;
      const start = listStart(list);
      return (
        <box key={key} flexDirection="column">
          {list.items.map((item, index) => {
            const marker = list.ordered ? `${start + index}. ` : "• ";
            return (
              <box key={blockKey(index, item.raw)} flexDirection="row">
                <text fg={palette.accent}>{marker}</text>
                {/* The item's own columns: a box that takes what the marker
                    leaves wraps its text at that width, and a box that grows
                    into the row wraps at the panel's. */}
                <box flexDirection="column" width={Math.max(1, columns - displayWidth(marker))}>
                  {item.tokens.map((inner, innerIndex) =>
                    block(inner, palette, ink, blockKey(innerIndex, inner.raw), columns),
                  )}
                </box>
              </box>
            );
          })}
        </box>
      );
    }
    case "code": {
      const code = token as Tokens.Code;
      // A fence is one block of the answer, not a quote of it: the panel is the
      // same frame the tool blocks wear, filled with the page's raised wash.
      return (
        <Pane
          key={key}
          paddingLeft={2}
          paddingRight={2}
          borderColor={palette.border}
          background={palette.codeSurface}
        >
          <text wrapMode="word" fg={palette.markdownCode}>
            {code.text}
          </text>
        </Pane>
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
            block(inner, palette, ink, blockKey(innerIndex, inner.raw), columns),
          )}
        </box>
      );
    }
    case "table": {
      // A table is a table, not the pipes a reader would have to count. The
      // brick owns the widths, so the header and the body line up on the same
      // columns and a narrow page drops a column whole instead of squeezing it.
      const table = token as Tokens.Table;
      // A column's width comes from the page, not from the cells the table has
      // so far: a table that streamed in row by row would otherwise widen under
      // the rows already printed above it.
      const share = Math.max(
        3,
        Math.floor((columns - (table.header.length - 1)) / table.header.length),
      );
      const tableColumns: TableColumn[] = table.header.map((_cell, index) => ({
        key: `c${index}`,
        width: share,
        align: table.align[index] === "right" ? "right" : "left",
        hideOrder: index === 0 ? 0 : table.header.length - index,
      }));
      const rows = [
        { key: "head", cells: tableCells(table.header) },
        ...table.rows.map((cells, index) => ({ key: `row${index}`, cells: tableCells(cells) })),
      ];
      return (
        <Table
          key={key}
          columns={tableColumns}
          rows={rows}
          renderCell={(_column, _cell, row) =>
            row.key === "head" ? { ink: palette.markdownHeading, bold: true } : { ink }
          }
        />
      );
    }
    case "space":
      return <box key={key} height={1} />;
    case "hr":
      return <Rule key={key} color={palette.inkFaint} />;
    case "html":
      return null;
    default: {
      const raw = "raw" in token ? token.raw : "";
      return (
        <text key={key} wrapMode="word" fg={ink}>
          {raw}
        </text>
      );
    }
  }
}

/**
 * Renders one source, parsed once.
 *
 * The parse is memoized on the source: a re-render that hands the same text back
 * - a committed row repainted because the theme changed, say - paints the tokens
 * it already has instead of reading the text again.
 */
export const MarkdownBlock = memo(function MarkdownBlock({
  source,
  palette,
  ink,
}: {
  source: string;
  palette: MarkdownPalette;
  ink?: string;
}) {
  const tokens = useMemo(() => marked.lexer(source), [source]);
  const columns = useStaticColumns();
  const body = ink ?? palette.ink;
  return (
    <box flexDirection="column">
      {tokens.map((token, index) =>
        block(token, palette, body, blockKey(index, token.raw), columns),
      )}
    </box>
  );
});

/** Renders one whole markdown document. */
export function Markdown({
  source,
  palette,
  ink,
}: {
  source: string;
  palette: MarkdownPalette;
  ink?: string;
}) {
  return <MarkdownBlock source={source} palette={palette} ink={ink} />;
}

/**
 * Renders one streamed answer.
 *
 * Blocks are laid out one after another, so a paragraph wraps by word and a
 * fenced block keeps its own lines: the two decisions a terminal reader notices
 * first. The settled prefix is one source and the block still being written is
 * another, so a frame re-parses the tail and leaves everything before it alone.
 */
export function MarkdownBlocks({
  source,
  palette,
  ink,
}: {
  source: string;
  palette: MarkdownPalette;
  ink?: string;
}) {
  const { closed, open } = splitBlocks(source);
  // One column holds both parses: a streamed answer is one run of rows, so the
  // spacing around it belongs to the row, not to the seam between two parses.
  return (
    <box flexDirection="column">
      {closed.length === 0 ? null : <MarkdownBlock source={closed} palette={palette} ink={ink} />}
      {open.length === 0 ? null : <MarkdownBlock source={open} palette={palette} ink={ink} />}
    </box>
  );
}
