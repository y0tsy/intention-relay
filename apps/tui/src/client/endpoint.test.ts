//! Endpoint resolution: the platform paths and the declared instance rule.

import assert from "node:assert/strict";
import { join } from "node:path";
import test from "node:test";
import {
  DEFAULT_INSTANCE,
  endpointFor,
  isSafeInstance,
  resolveEndpoint,
  stateDirectory,
} from "./endpoint";
import { ClientFailure } from "./errors";

test("the linux endpoint follows the runtime directory chain", () => {
  const endpoint = endpointFor("intention-relay", {
    platform: "linux",
    env: { XDG_RUNTIME_DIR: "/run/user/1000", XDG_CONFIG_HOME: "/home/user/.config" },
  });
  assert.equal(endpoint.address, join("/run/user/1000", "intention-relay.sock"));
  assert.equal(endpoint.instanceId, "intention-relay");
});

test("a relative or absent runtime directory falls through to the config home", () => {
  assert.equal(
    endpointFor("relay", {
      platform: "linux",
      env: { XDG_RUNTIME_DIR: "run", HOME: "/home/user" },
    }).address,
    join("/home/user/.config", "intention-relay", "relay.sock"),
  );
  assert.equal(
    endpointFor("relay", { platform: "linux", env: { XDG_CONFIG_HOME: "/cfg" } }).address,
    join("/cfg", "intention-relay", "relay.sock"),
  );
  assert.equal(
    endpointFor("relay", { platform: "linux", env: { HOME: "/home/user" } }).address,
    join("/home/user", ".config", "intention-relay", "relay.sock"),
  );
});

test("the macOS endpoint lives in application support", () => {
  assert.equal(
    endpointFor("relay", { platform: "darwin", env: { HOME: "/Users/user" } }).address,
    join("/Users/user", "Library", "Application Support", "intention-relay", "relay.sock"),
  );
});

test("the windows endpoint is a named pipe", () => {
  assert.equal(endpointFor("relay", { platform: "win32", env: {} }).address, "\\\\.\\pipe\\relay");
});

test("a declared instance wins over the platform default", () => {
  const endpoint = resolveEndpoint({
    platform: "linux",
    env: { INTENTION_ENDPOINT: "second", XDG_RUNTIME_DIR: "/run/user/1000" },
  });
  assert.equal(endpoint.instanceId, "second");
  assert.equal(
    resolveEndpoint({ platform: "linux", env: { XDG_RUNTIME_DIR: "/run/user/1000" } }).instanceId,
    DEFAULT_INSTANCE,
  );
  assert.equal(
    resolveEndpoint({
      platform: "linux",
      env: { INTENTION_ENDPOINT: "   ", XDG_RUNTIME_DIR: "/run/user/1000" },
    }).instanceId,
    DEFAULT_INSTANCE,
    "a blank declared instance falls back to the platform default",
  );
});

test("an endpoint with no resolvable directory is unavailable", () => {
  assert.throws(
    () => endpointFor("relay", { platform: "linux", env: {} }),
    (error: unknown) =>
      error instanceof ClientFailure && error.dto.code === "local_runtime_directory_unavailable",
  );
});

test("an unsafe instance identifier is rejected before it becomes a file name", () => {
  for (const unsafe of [
    "",
    "Upper",
    "-lead",
    "trail-",
    "with/slash",
    "with space",
    "a".repeat(65),
  ]) {
    assert.throws(
      () => endpointFor(unsafe, { platform: "linux", env: { XDG_RUNTIME_DIR: "/run" } }),
      (error: unknown) =>
        error instanceof ClientFailure && error.dto.code === "invalid_local_endpoint_instance",
      unsafe,
    );
  }
  for (const safe of ["a", "intention-relay", "relay2", "a-b-c"]) {
    assert.equal(isSafeInstance(safe), true, safe);
  }
});

test("the state directory holds the launch lock beside the endpoint", () => {
  assert.equal(
    stateDirectory({ platform: "linux", env: { XDG_RUNTIME_DIR: "/run/user/1000" } }),
    "/run/user/1000",
  );
  assert.equal(
    stateDirectory({ platform: "darwin", env: { HOME: "/Users/user" } }),
    join("/Users/user", "Library", "Application Support", "intention-relay"),
  );
  assert.equal(
    stateDirectory({ platform: "win32", env: { APPDATA: "C:\\Users\\user\\AppData\\Roaming" } }),
    "C:\\Users\\user\\AppData\\Roaming\\intention-relay",
  );
});
