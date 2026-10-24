//! The wire: framing, decoding, and the deadline every call carries.

import assert from "node:assert/strict";
import test from "node:test";
import { withDeadline } from "./connection";
import { ClientFailure, errorDto } from "./errors";
import { decodeDaemonMessage, encodeRequest, subscribedSnapshot } from "./protocol";

test("a request frames its identity beside its tagged payload", () => {
  assert.deepEqual(encodeRequest(1, { kind: "get_daemon_health" }), {
    id: 1,
    request: { kind: "get_daemon_health" },
  });
});

test("a daemon message decodes by its discriminator", () => {
  const message = decodeDaemonMessage(
    '{"kind":"reply","data":{"id":1,"result":{"kind":"daemon_health","data":{}}}}',
  );
  assert.equal(message.kind, "reply");
});

test("a line this client cannot decode is rejected", () => {
  for (const line of ["", "not json", '{"kind":"unknown"}', "[]"]) {
    assert.throws(
      () => decodeDaemonMessage(line),
      (error: unknown) =>
        error instanceof ClientFailure && error.dto.code === "invalid_local_protocol_message",
      line,
    );
  }
});

test("a subscribe reply that answers another request is a mismatched response", () => {
  assert.throws(
    () => subscribedSnapshot({ kind: "daemon_health", data: {} }),
    (error: unknown) =>
      error instanceof ClientFailure && error.dto.code === "invalid_local_protocol_response",
  );
});

test("a deadline that passes first reports its own failure", async () => {
  await assert.rejects(
    withDeadline(
      () => new Promise<void>(() => undefined),
      5,
      () => errorDto("unavailable", "test_deadline", "the deadline passed"),
    ),
    (error: unknown) => error instanceof ClientFailure && error.dto.code === "test_deadline",
  );
});

test("an operation that answers before its deadline is returned unchanged", async () => {
  assert.equal(
    await withDeadline(
      async () => 7,
      50,
      () => errorDto("unavailable", "test", "unused"),
    ),
    7,
  );
});
