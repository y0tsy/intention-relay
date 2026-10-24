//! Where the daemon listens on this machine.
//!
//! The rules are the Rust transport's: one endpoint per instance identifier,
//! rooted in the per-user runtime directory on Linux, in the user's application
//! support directory on macOS, and behind a named pipe on Windows. A declared
//! `INTENTION_ENDPOINT` instance wins over the platform default, so a second
//! daemon on one machine is addressed instead of competed with.

import { posix, win32 } from "node:path";
import { fail, invalidInstance, runtimeDirectoryUnavailable } from "./errors";

/** The environment variable that declares one local endpoint instance. */
export const ENDPOINT_VARIABLE = "INTENTION_ENDPOINT";

/// The instance identifier every installation uses unless one is declared.
export const DEFAULT_INSTANCE = "intention-relay";

/** One resolved endpoint: the address to dial and the instance it belongs to. */
export interface LocalEndpoint {
  /** The instance identifier the endpoint was named with. */
  readonly instanceId: string;
  /**
   * The address this platform dials.
   *
   * A filesystem socket path on Unix and a `\\.\pipe\…` name on Windows, so a
   * caller never decides how the platform addresses a local socket.
   */
  readonly address: string;
}

/** The process facts endpoint resolution reads. */
export interface EndpointEnvironment {
  /** The platform, defaulting to the running one. */
  readonly platform?: NodeJS.Platform;
  /** The environment, defaulting to the process's own. */
  readonly env?: NodeJS.ProcessEnv;
}

/** Resolves the endpoint this process addresses, honouring a declared instance. */
export function resolveEndpoint(environment: EndpointEnvironment = {}): LocalEndpoint {
  const env = environment.env ?? process.env;
  const declared = env[ENDPOINT_VARIABLE];
  const instanceId =
    declared !== undefined && declared.trim().length > 0 ? declared : DEFAULT_INSTANCE;
  return endpointFor(instanceId, environment);
}

/**
 * Resolves the endpoint one instance identifier names.
 *
 * @throws {ClientFailure} A typed unavailable failure when the platform
 * directory cannot be determined, or a validation failure when the identifier
 * is not a safe name.
 */
export function endpointFor(
  instanceId: string,
  environment: EndpointEnvironment = {},
): LocalEndpoint {
  const platform = environment.platform ?? process.platform;
  const env = environment.env ?? process.env;
  if (!isSafeInstance(instanceId)) {
    fail(invalidInstance(instanceId));
  }
  if (platform === "win32") {
    return { instanceId, address: `\\\\.\\pipe\\${instanceId}` };
  }
  const base = platform === "darwin" ? applicationSupportDirectory(env) : runtimeDirectory(env);
  return { instanceId, address: posix.join(base, `${instanceId}.sock`) };
}

/**
 * Returns the directory a bootstrap lock lives in.
 *
 * The lock serializes daemon launches per endpoint, so it sits beside the
 * endpoint itself on Unix and in the roaming application data on Windows.
 */
export function stateDirectory(environment: EndpointEnvironment = {}): string {
  const platform = environment.platform ?? process.platform;
  const env = environment.env ?? process.env;
  if (platform === "win32") {
    const appData = absoluteOrNull(env.APPDATA, win32);
    if (appData === null) {
      fail(runtimeDirectoryUnavailable());
    }
    return win32.join(appData, "intention-relay");
  }
  return platform === "darwin" ? applicationSupportDirectory(env) : runtimeDirectory(env);
}

/**
 * Reports whether one string is a safe endpoint instance identifier.
 *
 * A safe name is a short run of lowercase letters, digits, and inner dashes: it
 * becomes a file name on Unix and a pipe name on Windows, and the daemon takes
 * the same rule, so an identifier this client accepts is one the daemon binds.
 */
export function isSafeInstance(instanceId: string): boolean {
  return /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/.test(instanceId);
}

/** Resolves the Linux runtime directory chain. */
function runtimeDirectory(env: NodeJS.ProcessEnv): string {
  const runtime = absoluteOrNull(env.XDG_RUNTIME_DIR, posix);
  if (runtime !== null) {
    return runtime;
  }
  const config = absoluteOrNull(env.XDG_CONFIG_HOME, posix);
  if (config !== null) {
    return posix.join(config, "intention-relay");
  }
  const home = absoluteOrNull(env.HOME, posix);
  if (home === null) {
    fail(runtimeDirectoryUnavailable());
  }
  return posix.join(home, ".config", "intention-relay");
}

/** Resolves the macOS application support directory. */
function applicationSupportDirectory(env: NodeJS.ProcessEnv): string {
  const home = absoluteOrNull(env.HOME, posix);
  if (home === null) {
    fail(runtimeDirectoryUnavailable());
  }
  return posix.join(home, "Library", "Application Support", "intention-relay");
}

/** Returns one absolute environment path, or `null` when it is absent or relative. */
function absoluteOrNull(
  value: string | undefined,
  flavour: typeof posix | typeof win32,
): string | null {
  if (value === undefined || value.length === 0 || !flavour.isAbsolute(value)) {
    return null;
  }
  return value;
}
