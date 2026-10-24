//! The process entry point: one renderer, one store, one application.

import { fileURLToPath } from "node:url";

import { createRoot } from "@opentui/react";
import { AppStore } from "./app/store";
import { IntentionClient } from "./client/client";
import { ProcessDaemonLauncher } from "./client/daemon";
import { resolveEndpoint } from "./client/endpoint";
import { closeScreen, createSplitScreen, pushScreen } from "./kit";
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

// The window owns the last rows of the screen, so what the shell printed above
// the launch is pushed into the terminal's history before the panel paints over
// it: the rows stay in the scrollback and the window starts empty.
if (process.stdout.isTTY) {
  process.stdout.write(pushScreen(process.stdout.rows));
}
// The window is a footer: the transcript is printed into the terminal's own
// scrollback, so the wheel, the scrollbar, and a drag belong to the terminal and
// the application never asks for the mouse.
const renderer = await createSplitScreen();
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

/**
 * Leaves the terminal while keeping what it has printed.
 *
 * The frame's bottom edge is the window's last row, so the shell's prompt needs
 * a row of its own under it; the rows above stay in the terminal's history and
 * the saved lines are never dropped.
 */
async function quit(): Promise<void> {
  await closeScreen(renderer);
  process.exit(0);
}

root.render(<App store={store} />);
// The first frame is painted before the daemon is reached, so a slow bootstrap
// shows the welcome surface instead of a blank terminal.
store.dispatch({ kind: "connect_requested" });
