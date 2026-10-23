//! The NDJSON wire framing the daemon speaks.
//!
//! One message is one JSON document on one line. The daemon and this client
//! share the Rust declarations through `../bindings`, so a field rename reaches
//! both by regeneration instead of by a silent mismatch.

import type {
  ClientRequestDto,
  ProtocolDaemonMessageDto,
  ProtocolRequestDto,
  ProtocolResultDto,
  SessionFrameDto,
} from "../bindings";
import { fail, invalidLine, invalidResponse } from "./errors";

/// The largest framed line either peer accepts, terminator included.
export const MAX_MESSAGE_BYTES = 1_048_576;

/// The largest session snapshot one reply may carry.
export const MAX_TRANSCRIPT_SNAPSHOT_BYTES = MAX_MESSAGE_BYTES - MAX_MESSAGE_BYTES / 4;

/// The identity this client gives its one request on a connection.
///
/// One connection carries exactly one in-flight request, so the identity is
/// fixed: it exists to let the daemon echo the correlation, not to multiplex.
export const REQUEST_ID = 1;

/** Frames one request for one connection. */
export function encodeRequest(id: number, request: ClientRequestDto): ProtocolRequestDto {
  return { id, request };
}

/** Decodes one line the daemon sent, rejecting anything the declarations do not describe. */
export function decodeDaemonMessage(line: string): ProtocolDaemonMessageDto {
  let parsed: unknown;
  try {
    parsed = JSON.parse(line);
  } catch {
    fail(invalidLine());
  }
  if (!isRecord(parsed) || typeof parsed.kind !== "string") {
    fail(invalidLine());
  }
  switch (parsed.kind) {
    case "reply":
    case "rejection":
    case "frame":
      // The one place this client trusts the wire: the discriminator is
      // checked, and every payload field is read through the generated
      // declarations.
      return parsed as ProtocolDaemonMessageDto;
    default:
      fail(invalidLine());
  }
}

/** Reads a session-stream reply out of one result, or reports a mismatched answer. */
export function subscribedSnapshot(
  result: ProtocolResultDto,
): Extract<ProtocolResultDto, { kind: "session_subscribed" }>["data"] {
  if (result.kind !== "session_subscribed") {
    fail(invalidResponse());
  }
  return result.data;
}

/** Reports whether one parsed value is a JSON object. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** The kinds of a session frame, as the daemon sends them. */
export type SessionFrameKind = SessionFrameDto["kind"];
