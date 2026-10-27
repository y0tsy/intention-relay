//! Pairing the committed rows into the blocks a transcript draws.
//!
//! A tool exchange is one block drawn at the call's position, so the result row
//! it pairs with contributes no block of its own. A result without its call
//! stays a block of its own and says so.

import type { MessageProjectionDto, RunProjectionDto } from "../bindings";
import { runStatusIsTerminal } from "../client/stream";

/** One block of the transcript: the rows it draws and what decides their state. */
export interface TranscriptBlock {
  readonly key: string;
  readonly message: MessageProjectionDto | null;
  readonly call: MessageProjectionDto | null;
  readonly result: MessageProjectionDto | null;
}

/** The row a finished run closes with: the run whose facts it prints. */
export interface RunMetaBlock {
  readonly key: string;
  readonly run: RunProjectionDto;
}

/** One block the printer can commit: a transcript block or a run's closing row. */
export type PrintBlock = TranscriptBlock | RunMetaBlock;

/** Returns the call identity one tool row carries. */
export function callId(message: MessageProjectionDto): string | null {
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
export function transcriptBlocks(messages: readonly MessageProjectionDto[]): TranscriptBlock[] {
  const results = new Map<string, MessageProjectionDto>();
  const calls = new Set<string>();
  for (const message of messages) {
    const id = message.kind === "tool_result" ? callId(message) : null;
    if (id !== null) {
      results.set(id, message);
    }
    const call = message.kind === "tool_call" ? callId(message) : null;
    if (call !== null) {
      calls.add(call);
    }
  }
  return messages
    .filter((message) => {
      const id = message.kind === "tool_result" ? callId(message) : null;
      return id === null || !calls.has(id);
    })
    .map((message) => {
      const id = message.kind === "tool_call" ? callId(message) : null;
      const result = id === null ? null : (results.get(id) ?? null);
      const key = String(message.id);
      return id === null
        ? { key, message, call: null, result: null }
        : { key, message, call: message, result };
    });
}

/**
 * Adds the row a finished run closes with after the run's last block.
 *
 * The daemon commits no such row: the closing line prints the projection's own
 * facts, so the front end adds it once the run is terminal. A run whose rows the
 * snapshot does not carry gets no line, and every other block keeps its order.
 */
export function withRunMeta(
  blocks: readonly TranscriptBlock[],
  run: RunProjectionDto | null,
): PrintBlock[] {
  if (run === null || !runStatusIsTerminal(run.status)) {
    return [...blocks];
  }
  let last = -1;
  for (const [index, block] of blocks.entries()) {
    if (block.message?.run_id === run.run_id) {
      last = index;
    }
  }
  if (last < 0) {
    return [...blocks];
  }
  const meta: RunMetaBlock = { key: `meta:${run.run_id}`, run };
  return [...blocks.slice(0, last + 1), meta, ...blocks.slice(last + 1)];
}
