//! The process entry point: one renderer, one store, one application.

import { createCliRenderer } from "@opentui/core";
import { createRoot } from "@opentui/react";
import { fileURLToPath } from "node:url";
import { AppStore } from "./app/store";
import { IntentionClient } from "./client/client";
import { ProcessDaemonLauncher } from "./client/daemon";
import { resolveEndpoint } from "./client/endpoint";
import { configPath, readThemePreference, writeThemePreference } from "./theme/config";
import { App } from "./ui/App";

/**
 * Resolves the daemon program.
 *
 * A development checkout keeps the daemon in the workspace's Cargo target
 * directory beside the terminal app; `INTENTION_DAEMON_PROGRAM` overrides it for
 * an installation that keeps the two elsewhere.
 */
function daemonProgram(): string {
  const declared = process.env.INTENTION_DAEMON_PROGRAM;
  if (declared !== undefined && declared.length > 0) {
    return declared;
  }
  const suffix = process.platform === "win32" ? ".exe" : "";
  return fileURLToPath(new URL(`../../../target/debug/intention-daemon${suffix}`, import.meta.url));
}

const renderer = await createCliRenderer({ exitOnCtrlC: false });
const root = createRoot(renderer);

const theme = await readThemePreference(configPath());
const store = new AppStore({
  client: new IntentionClient(resolveEndpoint(), new ProcessDaemonLauncher(daemonProgram())),
  // A session runs in the directory the app was launched from, which the
  // launcher records before it moves to the app's own directory.
  workspaceRoot: process.env.INTENTION_WORKSPACE ?? process.cwd(),
  mode: null,
  initialSession: null,
  theme,
  recorder: {
    writeTheme: (preference) => writeThemePreference(preference, configPath()),
  },
  onQuit: () => void quit(),
});

/** Leaves the terminal exactly as it was found. */
async function quit(): Promise<void> {
  renderer.destroy();
  await renderer.closed;
  process.exit(0);
}

root.render(<App store={store} />);
// The first frame is painted before the daemon is reached, so a slow bootstrap
// shows the welcome surface instead of a blank terminal.
store.dispatch({ kind: "connect_requested" });
