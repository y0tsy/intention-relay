//! The surface a fresh session starts on.
//!
//! The block renders the composition its plan chose for the panel it was given:
//! whole lines only, never more rows than the panel holds, and the leftover rows
//! shared around the block instead of squashing its children.

import { TextAttributes } from "@opentui/core";
import { COMMANDS } from "../app/commands";
import type { Palette } from "../theme/palette";
import { commandRow, planRows, planWelcome } from "./welcome-plan";

/** Renders the empty transcript. */
export function Welcome({
  width,
  height,
  workspace,
  palette,
}: {
  width: number;
  height: number;
  workspace: string;
  palette: Palette;
}) {
  const textWidth = Math.max(8, Math.min(width - 4, 78));
  const plan = planWelcome({ width, height, workspace });
  const workspaceLine = `new sessions run in ${workspace}`;
  const free = height - planRows(plan, textWidth, workspaceLine);
  return (
    <box flexDirection="column" flexGrow={1} paddingLeft={1} paddingRight={1} gap={1}>
      {/* A roomy panel centres the block, which reads as deliberate; a tight one
          keeps it at the top so the last line still has a blank row under it. */}
      <box
        flexDirection="column"
        flexGrow={1}
        justifyContent={free >= 3 ? "center" : "flex-start"}
        gap={1}
      >
        <text fg={palette.ink} attributes={TextAttributes.BOLD}>
          intention relay
        </text>
        {plan.rule ? <text fg={palette.inkFaint}>{"─".repeat(Math.max(4, textWidth))}</text> : null}
        {plan.tagline === null ? null : (
          <text wrapMode="word" fg={palette.inkMuted}>
            {plan.tagline}
          </text>
        )}
        {plan.examples.length === 0 ? null : (
          <box flexDirection="column">
            {plan.examples.map((example) => (
              <text key={example} wrapMode="word" fg={palette.inkFaint}>
                {`- ${example}`}
              </text>
            ))}
          </box>
        )}
        {plan.commands === "none" ? null : plan.commands === "hint" ? (
          <text fg={palette.inkFaint}>/ commands</text>
        ) : (
          <box flexDirection="column">
            {COMMANDS.map((command) => {
              const [usage, summary] = commandRow(command, textWidth).split("  ");
              return (
                <text key={command.name} wrapMode="word" fg={palette.inkFaint}>
                  <span fg={palette.markdownLink}>{usage}</span>
                  {summary === undefined ? "" : `  ${summary}`}
                </text>
              );
            })}
          </box>
        )}
      </box>
      {plan.workspace ? (
        <text wrapMode="word" fg={palette.inkFaint}>
          {workspaceLine}
        </text>
      ) : null}
    </box>
  );
}
