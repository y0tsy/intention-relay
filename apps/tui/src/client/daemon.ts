//! Starting and reaching the local daemon.
//!
//! The daemon is a separate process the client launches on first use. Its
//! standard streams are detached, because the fullscreen front end owns the
//! terminal: anything the daemon writes to an inherited stream lands on top of a
//! painted frame and scrolls it. Readiness is a healthy reply on a fresh
//! connection, never the exit of the launch.
//!
//! A per-endpoint lock serializes launches, so two windows that start together
//! produce one daemon instead of two competitors for the same socket.

import { spawn } from "node:child_process";
import { constants } from "node:fs";
import { type FileHandle, mkdir, open, rm, stat } from "node:fs/promises";
import { join } from "node:path";
import type { LocalEndpoint } from "./endpoint";
import { stateDirectory } from "./endpoint";
import { daemonLaunchFailed, unavailable } from "./errors";

/** How long a launch may take to produce a ready daemon. */
export const STARTUP_TIMEOUT_MILLISECONDS = 3_000;

/** How long one readiness probe waits between attempts. */
export const STARTUP_RETRY_MILLISECONDS = 25;

/**
 * How long a lock older than this is treated as abandoned.
 *
 * A holder never keeps the lock longer than the startup budget, so a lock this
 * old belongs to a process that died while holding it.
 */
const STALE_LOCK_MILLISECONDS = STARTUP_TIMEOUT_MILLISECONDS * 2;

/** Provides a daemon program and starts it for one endpoint. */
export interface DaemonLauncher {
  /** The program this launcher starts. */
  readonly program: string;
  /** Starts one daemon host for the endpoint. */
  launch(endpoint: LocalEndpoint): void;
}

/** A launcher for the thin `intention-daemon` binary beside the front end. */
export class ProcessDaemonLauncher implements DaemonLauncher {
  readonly program: string;

  constructor(program: string) {
    if (program.trim().length === 0) {
      throw new Error("a daemon program must not be empty");
    }
    this.program = program;
  }

  /**
   * Starts the daemon with every standard stream detached.
   *
   * @throws {ClientFailure} The typed launch failure when the program cannot be
   * spawned.
   */
  launch(endpoint: LocalEndpoint): void {
    try {
      const child = spawn(this.program, [endpoint.instanceId], {
        detached: true,
        stdio: "ignore",
        windowsHide: true,
      });
      // The launch is fire-and-forget: readiness is read from the endpoint, and
      // the child outlives this process by design, so it is never waited on.
      child.unref();
    } catch {
      throw daemonLaunchFailed();
    }
  }
}

/**
 * Takes the per-endpoint launch lock, waiting until the deadline passes.
 *
 * @throws {ClientFailure} The typed unavailable failure when the lock stays
 * held for the whole budget.
 */
export async function acquireStartupLock(
  endpoint: LocalEndpoint,
  budget: number = STARTUP_TIMEOUT_MILLISECONDS,
): Promise<StartupLock> {
  const directory = stateDirectory();
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const path = join(directory, `bootstrap-${endpoint.instanceId}.lock`);
  const deadline = Date.now() + budget;
  for (;;) {
    try {
      const handle = await open(
        path,
        constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY,
        0o600,
      );
      return new StartupLock(handle, path);
    } catch (error) {
      if (!isAlreadyHeld(error)) {
        throw unavailable("startup_lock_unavailable");
      }
      await reclaimIfAbandoned(path);
      if (Date.now() >= deadline) {
        throw unavailable("startup_lock_unavailable");
      }
      await sleep(STARTUP_RETRY_MILLISECONDS);
    }
  }
}

/** One held launch lock, released when the bootstrap ends. */
export class StartupLock {
  readonly #handle: FileHandle;
  readonly #path: string;
  #released = false;

  constructor(handle: FileHandle, path: string) {
    this.#handle = handle;
    this.#path = path;
  }

  /** Releases the lock; releasing twice is a no-op. */
  async release(): Promise<void> {
    if (this.#released) {
      return;
    }
    this.#released = true;
    await this.#handle.close().catch(() => undefined);
    await rm(this.#path, { force: true }).catch(() => undefined);
  }
}

/** Reports whether a lock file error means another holder has the lock. */
function isAlreadyHeld(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code: unknown }).code === "EEXIST"
  );
}

/** Removes a lock whose holder died while holding it. */
async function reclaimIfAbandoned(path: string): Promise<void> {
  const age = await lockAgeMilliseconds(path);
  if (age !== null && age > STALE_LOCK_MILLISECONDS) {
    await rm(path, { force: true }).catch(() => undefined);
  }
}

/** Returns how long a lock file has existed, or `null` when it is gone. */
async function lockAgeMilliseconds(path: string): Promise<number | null> {
  try {
    const details = await stat(path);
    return Date.now() - details.mtimeMs;
  } catch {
    return null;
  }
}

/** Waits one retry interval. */
export function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });
}
