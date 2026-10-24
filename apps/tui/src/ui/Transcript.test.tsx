//! The transcript: one committed block per row, the step in flight after them.
//!
//! A committed block is printed into the terminal's scrollback and the step in
//! flight stays in the footer; both are built from the same components, so the
//! test paints them in the column the screen shows.

import assert from "node:assert/strict";
import test from "node:test";
import { row } from "../testing/fixtures";
import { frameLines, painted } from "../testing/render";
import { offlineStore, openSession } from "../testing/store";
import { DARK } from "../theme/palette";
import { ContentColumns, Stack } from "../kit";
import { transcriptBlocks } from "../app/transcript";
import { CommittedBlock, Live } from "./Transcript";

/** Renders one transcript at one width and returns its lines. */
async function lines(
  messages: ReturnType<typeof row>[],
  options: {
    reasoning?: string;
    answer?: string;
    reasoningExpansion?: Record<string, number>;
    width?: number;
    height?: number;
  } = {},
): Promise<string[]> {
  const store = offlineStore();
  openSession(store, {
    messages,
    answer: options.answer,
    reasoning: options.reasoning,
  });
  if (options.reasoningExpansion !== undefined) {
    // A reader reveals a block through the same gesture, wherever it sits.
    for (const [key, chunks] of Object.entries(options.reasoningExpansion)) {
      for (let chunk = 0; chunk < chunks; chunk += 1) {
        const id = key === "live" ? null : key;
        store.dispatch({ kind: "reasoning_expanded", id });
      }
    }
  }
  // The window prints the committed rows and keeps the step in flight in the
  // footer, so the two views sit in one column the way the screen reads.
  const width = options.width ?? 58;
  const expansion = options.reasoningExpansion ?? {};
  const frame = await painted(
    <Stack spaced>
      <ContentColumns columns={width}>
        <Stack spaced>
          {transcriptBlocks(messages).map((block) => (
            <CommittedBlock
              key={block.key}
              block={block}
              palette={DARK}
              expansion={expansion[block.key] ?? 0}
            />
          ))}
        </Stack>
      </ContentColumns>
      <ContentColumns columns={width}>
        <Live store={store} palette={DARK} />
      </ContentColumns>
    </Stack>,
    options.width ?? 60,
    options.height ?? 30,
  );
  store.dispose();
  return frameLines(frame);
}

test("a committed turn reads as its label, its reasoning, and its answer", async () => {
  const painted_ = await lines([
    row(1, "user", "explain this repository"),
    { ...row(2, "assistant", "It relays intentions."), reasoning: "they asked" },
  ]);
  assert.ok(
    painted_.some((line) => line === "you"),
    "the reader's turn opens with its label",
  );
  assert.ok(painted_.some((line) => line.includes("explain this repository")));
  assert.ok(painted_.some((line) => line.startsWith("∴ reasoning")));
  assert.ok(
    painted_.some((line) => line === "assistant"),
    "the answer carries its label",
  );
  assert.ok(painted_.some((line) => line === "It relays intentions."));
});

test("a long reasoning opens collapsed and reveals its rows on a gesture", async () => {
  const reasoning = Array.from({ length: 20 }, (_, index) => `thought ${index + 1}`).join("\n");
  const bounded = await lines([{ ...row(2, "assistant", "an answer"), reasoning }]);
  assert.ok(bounded.some((line) => /… 20 more · click or Ctrl\+E/.test(line)));
  const expanded = await lines([{ ...row(2, "assistant", "an answer"), reasoning }], {
    reasoningExpansion: { "2": 20 },
    height: 60,
  });
  assert.ok(expanded.some((line) => line.trim() === "thought 20"));
  assert.ok(!expanded.some((line) => /more · click or Ctrl\+E/.test(line)));
});

test("the step in flight reads as the reasoning and the answer it becomes", async () => {
  const painted_ = await lines([], { reasoning: "...", answer: "hold on" });
  assert.ok(painted_.some((line) => line.startsWith("∴ reasoning")));
  assert.ok(painted_.some((line) => line === "assistant"));
  assert.ok(painted_.some((line) => line === "hold on"));
});

test("the step in flight borrows the expansion of the row it becomes", async () => {
  const reasoning = Array.from({ length: 20 }, (_, index) => `thought ${index + 1}`).join("\n");
  const painted_ = await lines([], {
    reasoning,
    answer: "hold on",
    reasoningExpansion: { live: 2 },
  });
  assert.ok(painted_.some((line) => line.trim() === "thought 2"));
});

test("a call and its result are one block drawn at the call", async () => {
  const call = {
    ...row(3, "tool_call", ""),
    tool: { row: "call", value: { call_id: "c1", tool_id: "grep", arguments: null } },
  } as never;
  const result = {
    ...row(4, "tool_result", "the pattern matched twice"),
    tool: {
      row: "result",
      value: { call_id: "c1", tool_id: "grep", status: "completed", result: null, metadata: [] },
    },
  } as never;
  const painted_ = await lines([call, result]);
  assert.equal(
    painted_.filter((line) => line.includes("▸ grep")).length,
    1,
    "the exchange is drawn once, at the call",
  );
  assert.ok(painted_.some((line) => line.includes("‣ the pattern matched twice")));
});

test("an execute result never prints the command's output", async () => {
  const call = {
    ...row(3, "tool_call", ""),
    tool: { row: "call", value: { call_id: "c1", tool_id: "execute", arguments: null } },
  } as never;
  const result = {
    ...row(4, "tool_result", "pong\npong"),
    tool: {
      row: "result",
      value: { call_id: "c1", tool_id: "execute", status: "completed", result: null, metadata: [] },
    },
  } as never;
  const painted_ = await lines([call, result]);
  assert.ok(!painted_.some((line) => line.includes("pong")), "no byte of the output is shown");
});

test("a notice stays a plain line, never prose", async () => {
  const painted_ = await lines([row(1, "notice", "**not** markdown")]);
  assert.ok(painted_.some((line) => line.startsWith("※ **not** markdown")));
});

test("the blocks of one turn are separated by exactly one blank row", async () => {
  const painted_ = await lines([row(1, "user", "hi"), row(2, "assistant", "hello")]);
  const user = painted_.indexOf("you");
  const answer = painted_.indexOf("assistant");
  assert.ok(user >= 0 && answer > user, "the reader's turn opens and the answer follows");
  assert.deepEqual(
    painted_.slice(user, answer).filter((line) => line.trim().length === 0),
    [""],
    "one blank row, not two",
  );
});
