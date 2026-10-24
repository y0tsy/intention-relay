//! The typed failures the terminal app reports.
//!
//! Every failure a launch, connection, reply, or scope check can produce is one
//! of the declarations below. They mirror the Rust client's `ErrorDto` values,
//! including the codes, so a failure reads the same whether it came from the
//! command-line front end or from this one.

import type { ErrorCategoryDto, ErrorDto } from "../bindings";

/** Builds one typed failure with its category and safe message. */
export function errorDto(category: ErrorCategoryDto, code: string, message: string): ErrorDto {
  return {
    code,
    category,
    message,
    retryable: category === "unavailable",
  };
}

/**
 * One typed failure as a thrown value.
 *
 * Every failure this client reports is an `ErrorDto`, and this class is what
 * carries one out of a rejected promise: a caller reads `failure.dto` for the
 * code and category, and a test asserts on the declaration instead of on a
 * message.
 */
export class ClientFailure extends Error {
  /** The declaration this failure carries. */
  readonly dto: ErrorDto;

  constructor(dto: ErrorDto) {
    super(`${dto.code}: ${dto.message}`);
    this.name = "ClientFailure";
    this.dto = dto;
  }
}

/** Throws one typed failure. */
export function fail(dto: ErrorDto): never {
  throw new ClientFailure(dto);
}

/** Reports whether one failure means the daemon endpoint is not yet reachable. */
export function isUnavailable(error: ErrorDto): boolean {
  return error.category === "unavailable";
}

/** The failure for a message the client could not frame or decode. */
export function invalidLine(): ErrorDto {
  return errorDto(
    "validation",
    "invalid_local_protocol_message",
    "the local daemon sent a message this client cannot decode",
  );
}

/** The failure for a reply that does not answer the request it claimed. */
export function invalidResponse(): ErrorDto {
  return errorDto(
    "validation",
    "invalid_local_protocol_response",
    "the local daemon returned an unexpected protocol response",
  );
}

/** The failure for stream data scoped to another session. */
export function scopeError(): ErrorDto {
  return errorDto(
    "validation",
    "invalid_session_subscription",
    "session subscription data belongs to another session scope",
  );
}

/** The failure for a request the daemon never answered. */
export function requestTimeout(): ErrorDto {
  return errorDto(
    "unavailable",
    "local_daemon_reply_timeout",
    "the local daemon did not answer the request in time",
  );
}

/** The failure for a subscription request the daemon never answered. */
export function streamReplyTimeout(): ErrorDto {
  return errorDto(
    "unavailable",
    "session_stream_reply_timeout",
    "the local daemon did not answer the session-stream request in time",
  );
}

/** The failure for an endpoint the client cannot address. */
export function unavailable(code: string = "local_daemon_connection_unavailable"): ErrorDto {
  return errorDto("unavailable", code, "the local daemon connection is unavailable");
}

/** The failure for a daemon program the client cannot resolve. */
export function daemonProgramUnavailable(): ErrorDto {
  return errorDto(
    "unavailable",
    "daemon_program_unavailable",
    "the daemon program could not be resolved",
  );
}

/** The failure for a daemon process that would not start. */
export function daemonLaunchFailed(): ErrorDto {
  return errorDto(
    "unavailable",
    "local_daemon_launch_failed",
    "the local daemon could not be started",
  );
}

/** The failure for an instance identifier that is not a safe endpoint name. */
export function invalidInstance(instanceId: string): ErrorDto {
  return errorDto(
    "validation",
    "invalid_local_endpoint_instance",
    `the endpoint instance identifier ${JSON.stringify(instanceId)} is not a safe name`,
  );
}

/** The failure for a platform with no resolvable endpoint directory. */
export function runtimeDirectoryUnavailable(): ErrorDto {
  return errorDto(
    "unavailable",
    "local_runtime_directory_unavailable",
    "the local endpoint directory could not be determined",
  );
}

/** The failure for a bootstrap that never reached ready health. */
export function daemonNotReady(): ErrorDto {
  return errorDto(
    "unavailable",
    "local_daemon_not_ready",
    "the local daemon did not report ready health in time",
  );
}
