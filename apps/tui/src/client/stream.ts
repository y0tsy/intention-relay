//! The committed state of one session as the daemon reports it.
//!
//! This is the port of the Rust client's `SessionStreamState`: the rows, the
//! active run projection, and the transient text of the model step in flight.
//! The rows are bounded exactly like the Rust client's, so a long-lived
//! subscription never grows without limit; the durable transcript stays complete
//! in the daemon, and a re-subscribe re-reads the current snapshot.

import type {
  MessageId,
  MessageProjectionDto,
  RunProjectionDto,
  RunStatusDto,
  SessionFrameDto,
  SessionId,
  TextDeltaChannelDto,
} from "../bindings";
import { fail, scopeError } from "./errors";

/** The number of newest committed transcript rows one subscription retains. */
export const RETAINED_TRANSCRIPT_MESSAGES = 256;

/** What one applied session-stream frame changed in the committed state. */
export type FrameUpdate =
  | { readonly kind: "unchanged" }
  | { readonly kind: "row"; readonly row: MessageId }
  | { readonly kind: "run"; readonly run: RunProjectionDto["run_id"] }
  | {
      readonly kind: "delta";
      readonly runId: RunProjectionDto["run_id"];
      readonly step: number;
    };

/** Reports whether one run status is terminal. */
export function runStatusIsTerminal(status: RunStatusDto): boolean {
  return status === "completed" || status === "failed" || status === "interrupted";
}

/** The committed state of one fixed session scope. */
export class SessionStreamState {
  readonly #sessionId: SessionId;
  #run: RunProjectionDto | null = null;
  #messages: MessageProjectionDto[] = [];
  #provisionalStep: number | null = null;
  #provisionalText = "";
  #provisionalReasoning = "";
  /**
   * How many times the committed rows have moved.
   *
   * Rows are mutated in place, so a view that compares the array it was handed
   * cannot tell an appended row from the same array read twice. A view keys the
   * identity of what it renders on this number instead: transient text moves
   * without moving it.
   */
  #revision = 0;

  private constructor(sessionId: SessionId) {
    this.#sessionId = sessionId;
  }

  /** Creates empty local state fixed to one session. */
  static empty(sessionId: SessionId): SessionStreamState {
    return new SessionStreamState(sessionId);
  }

  /**
   * Reads one durable session snapshot into a state of its own.
   *
   * The rows are the newest [`RETAINED_TRANSCRIPT_MESSAGES`] of the snapshot:
   * the snapshot is the daemon's read of the whole transcript, and this state
   * keeps the window it renders.
   */
  static fromSnapshot(snapshot: {
    session_id: SessionId;
    projection: { active_run?: RunProjectionDto | null };
    messages: MessageProjectionDto[];
  }): SessionStreamState {
    const state = new SessionStreamState(snapshot.session_id);
    state.#run = snapshot.projection.active_run ?? null;
    state.#messages = snapshot.messages.slice();
    state.#retainNewest();
    return state;
  }

  /** Merges a newer read of this session's durable transcript into this state. */
  reconcile(read: SessionStreamState): void {
    this.#ensureScope(read.#sessionId);
    const known = new Set(this.#messages.map((row) => row.id));
    for (const row of read.#messages) {
      if (!known.has(row.id)) {
        this.#messages.push(row);
      }
    }
    this.#messages.sort((left, right) => left.id - right.id);
    this.#run = read.#run;
    // The read is a fresh observation of committed state: the transient
    // advance notice of an earlier stream never outlives it.
    this.#clearProvisional();
    this.#retainNewest();
    this.#touch();
  }

  /**
   * Applies one uncorrelated session-stream frame and reports what it changed.
   *
   * @throws {ClientFailure} The scoped-response failure for a frame that belongs
   * to another session; the state is unchanged.
   */
  applyFrame(frame: SessionFrameDto): FrameUpdate {
    switch (frame.kind) {
      case "content":
        return this.#applyContent(frame.data);
      case "status":
        return this.#applyStatus(frame.data);
      case "text_delta":
        return this.#applyTextDelta(frame.data);
    }
  }

  /** Ends this state's stream observation, dropping the transient text. */
  endStream(): void {
    this.#clearProvisional();
  }

  /** Returns the fixed session identity. */
  get sessionId(): SessionId {
    return this.#sessionId;
  }

  /** Returns the current run projection. */
  get run(): RunProjectionDto | null {
    return this.#run;
  }

  /** Returns the current run lifecycle status. */
  get status(): RunStatusDto | null {
    return this.#run?.status ?? null;
  }

  /** Returns the committed transcript rows accepted so far. */
  get messages(): readonly MessageProjectionDto[] {
    return this.#messages;
  }

  /** Returns how many times the committed rows have moved. */
  get revision(): number {
    return this.#revision;
  }

  /** Returns the transient answer text of the current model step. */
  get provisionalText(): string {
    return this.#provisionalText;
  }

  /** Returns the transient reasoning text of the current model step. */
  get provisionalReasoning(): string {
    return this.#provisionalReasoning;
  }

  #applyContent(message: MessageProjectionDto): FrameUpdate {
    this.#ensureScope(message.session_id);
    // A content frame repeating the newest accepted row carries the identity
    // the subscription snapshot already carried; re-appending it would show
    // the same committed row twice.
    const repeated = this.#messages.at(-1)?.id === message.id;
    if (message.kind === "assistant") {
      this.#clearProvisional();
    }
    if (repeated) {
      return { kind: "unchanged" };
    }
    this.#messages.push(message);
    this.#retainNewest();
    this.#touch();
    return { kind: "row", row: message.id };
  }

  #applyTextDelta(delta: {
    session_id: SessionId;
    run_id: RunProjectionDto["run_id"];
    step: number;
    channel: TextDeltaChannelDto;
    text: string;
  }): FrameUpdate {
    this.#ensureScope(delta.session_id);
    // Transient text belongs to the session's current run: a delta of a run
    // this state no longer projects crossed its run's own terminal commit, so
    // it changes nothing.
    if (this.#run === null || this.#run.run_id !== delta.run_id) {
      return { kind: "unchanged" };
    }
    // A delta for a new step replaces both buffers: the previous step's
    // advance notice is stale once the run moves on.
    if (this.#provisionalStep !== delta.step) {
      this.#provisionalStep = delta.step;
      this.#provisionalText = "";
      this.#provisionalReasoning = "";
    }
    if (delta.channel === "answer") {
      this.#provisionalText += delta.text;
    } else {
      this.#provisionalReasoning += delta.text;
    }
    return { kind: "delta", runId: delta.run_id, step: delta.step };
  }

  #applyStatus(run: RunProjectionDto): FrameUpdate {
    this.#ensureScope(run.session_id);
    // A status frame replaces the whole projection: it is idempotent, and it is
    // how a window learns that a run started elsewhere.
    if (runStatusIsTerminal(run.status)) {
      // The daemon discards a run's unpublished windows when it ends, so no
      // advance notice may outlive the run.
      this.#clearProvisional();
    }
    const changed = this.#run === null || !sameRun(this.#run, run);
    this.#run = run;
    return changed ? { kind: "run", run: run.run_id } : { kind: "unchanged" };
  }

  /** Moves the row revision forward after the committed rows changed. */
  #touch(): void {
    this.#revision += 1;
  }

  /** Drops the transient text of every channel of the current model step. */
  #clearProvisional(): void {
    this.#provisionalStep = null;
    this.#provisionalText = "";
    this.#provisionalReasoning = "";
  }

  /** Drops the oldest retained rows past the retention bound. */
  #retainNewest(): void {
    const excess = this.#messages.length - RETAINED_TRANSCRIPT_MESSAGES;
    if (excess > 0) {
      this.#messages.splice(0, excess);
    }
  }

  /** Rejects data scoped to another session. */
  #ensureScope(sessionId: SessionId): void {
    if (sessionId !== this.#sessionId) {
      fail(scopeError());
    }
  }
}

/** Reports whether two projections of one run describe the same committed facts. */
function sameRun(left: RunProjectionDto, right: RunProjectionDto): boolean {
  return (
    left.run_id === right.run_id &&
    left.status === right.status &&
    left.started_at === right.started_at &&
    left.finished_at === right.finished_at &&
    left.open_tool_calls === right.open_tool_calls &&
    JSON.stringify(left.usage ?? null) === JSON.stringify(right.usage ?? null)
  );
}
