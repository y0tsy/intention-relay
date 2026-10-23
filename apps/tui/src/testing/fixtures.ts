//! Fixtures the tests build wire values from.
//!
//! Every builder takes only the fields a scenario is about and fills the rest
//! with values that read as themselves, so a test names the one fact it asserts
//! instead of restating a whole declaration.

import type {
  MessageId,
  MessageKindDto,
  MessageProjectionDto,
  RunId,
  RunProjectionDto,
  RunStatusDto,
  SessionId,
  SessionSnapshotDto,
  SessionSummaryDto,
  TextDeltaFrameDto,
  TurnId,
} from "../bindings";

/** The session every fixture belongs to unless a scenario names another. */
export const SESSION_ID: SessionId = "0b1a2c3d-0000-4000-8000-000000000001";

/** The run every fixture belongs to unless a scenario names another. */
export const RUN_ID: RunId = "0b1a2c3d-0000-4000-8000-0000000000a1";

/** The turn every fixture belongs to unless a scenario names another. */
export const TURN_ID: TurnId = "0b1a2c3d-0000-4000-8000-0000000000b1";

/** Returns one committed transcript row. */
export function row(
  id: MessageId,
  kind: MessageKindDto,
  text: string,
  overrides: Partial<MessageProjectionDto> = {},
): MessageProjectionDto {
  return {
    id,
    session_id: SESSION_ID,
    run_id: RUN_ID,
    kind,
    text,
    reasoning: null,
    tool: null,
    ...overrides,
  };
}

/** Returns one run projection. */
export function run(
  status: RunStatusDto,
  overrides: Partial<RunProjectionDto> = {},
): RunProjectionDto {
  const finished = status === "completed" || status === "failed" || status === "interrupted";
  return {
    session_id: SESSION_ID,
    run_id: RUN_ID,
    turn_id: TURN_ID,
    status,
    usage: null,
    started_at: 1_700_000_000,
    finished_at: finished ? 1_700_000_010 : null,
    open_tool_calls: 0,
    ...overrides,
  };
}

/** Returns one session snapshot. */
export function snapshot(
  messages: MessageProjectionDto[],
  activeRun: RunProjectionDto | null,
  sessionId: SessionId = SESSION_ID,
): SessionSnapshotDto {
  return {
    session_id: sessionId,
    projection: {
      project_id: "0b1a2c3d-0000-4000-8000-0000000000c1",
      session_id: sessionId,
      workspace_id: "0b1a2c3d-0000-4000-8000-0000000000d1",
      workspace_root: "/workspace",
      mode: "build",
      active_run: activeRun,
      provider_model: null,
    },
    messages,
  };
}

/** Returns one transient text delta. */
export function delta(
  step: number,
  channel: TextDeltaFrameDto["channel"],
  text: string,
): TextDeltaFrameDto {
  return { session_id: SESSION_ID, run_id: RUN_ID, step, channel, text };
}

/** Returns one session summary, as the browser's table reads it. */
export function summary(overrides: Partial<SessionSummaryDto> = {}): SessionSummaryDto {
  return {
    session_id: SESSION_ID,
    project_id: "0b1a2c3d-0000-4000-8000-0000000000c1",
    workspace_id: "0b1a2c3d-0000-4000-8000-0000000000d1",
    mode: "build",
    created_at: 1_700_000_000,
    updated_at: 1_700_000_060,
    workspace_root: "/workspace",
    message_count: 3,
    ...overrides,
  };
}
