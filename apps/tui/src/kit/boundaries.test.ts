//! The kernel boundary: what a surface may say and what only the kernel may.
//!
//! The kernel owns geometry, text measurement, the renderer, and the split
//! footer surface. A surface composes bricks and names content; it never reaches
//! for a terminal field, a display width, or an inner kernel module. The guard
//! reads the production sources, skips tests, and proves both directions: a
//! forbidden line is caught, and the sources it scans pass.

import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const SRC = fileURLToPath(new URL("..", import.meta.url));

/** Every production source under `src`, tests and bindings excluded. */
function productionSources(): string[] {
  const files: string[] = [];
  const walk = (path: string): void => {
    for (const entry of readdirSync(path)) {
      const full = join(path, entry);
      if (statSync(full).isDirectory()) {
        if (entry !== "bindings") {
          walk(full);
        }
        continue;
      }
      if (entry.endsWith(".ts") || entry.endsWith(".tsx")) {
        if (!entry.includes(".test.")) {
          files.push(full);
        }
      }
    }
  };
  for (const entry of ["ui", "app"]) {
    walk(join(SRC, entry));
  }
  files.push(join(SRC, "main.tsx"));
  return files;
}

/** The tokens a surface may not spell, and the reason it may not. */
const FORBIDDEN = [
  "useRenderer",
  "terminalHeight",
  "footerHeight",
  "createScrollbackSurface",
  "setRenderOffset",
  "pendingNativeMode",
  "externalOutputQueue",
  "displayWidth",
  ".repeat(",
  "\\u00a0",
  "footerLayout",
  "browserLayout",
  "welcomeLayout",
  "inputBlockRows",
  "printRoom",
] as const;

/** The layout props a surface may not pass: geometry belongs to the bricks. */
const LAYOUT_PROPS = [
  "height",
  "width",
  "flexDirection",
  "flexGrow",
  "flexBasis",
  "flexShrink",
  "gap",
  "padding",
  "paddingLeft",
  "paddingRight",
  "paddingTop",
  "paddingBottom",
  "paddingX",
  "paddingY",
  "margin",
  "marginLeft",
  "marginRight",
  "marginTop",
  "marginBottom",
  "marginX",
  "marginY",
  "border",
  "borderStyle",
] as const;

/** Returns the raw boxes and layout props one surface spells in JSX. */
function jsxViolations(source: string): string[] {
  const found = new Set<string>();
  for (const tag of source.match(/<[A-Za-z][^<>]*>/g) ?? []) {
    if (/^<box\b/.test(tag)) {
      found.add("<box");
    }
    if (/^<scrollbox\b/.test(tag)) {
      found.add("<scrollbox");
    }
    for (const prop of LAYOUT_PROPS) {
      if (new RegExp(`\\s${prop}\\s*=`).test(tag)) {
        found.add(`layout prop ${prop}`);
      }
    }
  }
  return [...found];
}

/** Returns the forbidden tokens one source spells. */
function violations(path: string, source: string): string[] {
  const found: string[] = FORBIDDEN.filter((token) => source.includes(token));
  if (path.includes(`${sep}ui${sep}`)) {
    found.push(...jsxViolations(source));
  }
  if (source.includes('from "../kit/')) {
    found.push("a deep kernel import");
  }
  // The clock is the one surface that may do arithmetic: seconds since the
  // epoch is not geometry. App state may count; a surface may not.
  if (
    path.includes(`${sep}ui${sep}`) &&
    path.endsWith(`${sep}use-now.ts`) === false &&
    /\bMath\./.test(source)
  ) {
    found.push("Math.");
  }
  return found;
}

/** Returns every kernel source that reaches outside the kernel. */
function kernelEscapes(): string[] {
  const escapes: string[] = [];
  const walk = (path: string): void => {
    for (const entry of readdirSync(path)) {
      const full = join(path, entry);
      if (statSync(full).isDirectory()) {
        walk(full);
        continue;
      }
      if ((!entry.endsWith(".ts") && !entry.endsWith(".tsx")) || entry.includes(".test.")) {
        continue;
      }
      const source = readFileSync(full, "utf8");
      if (/\bfrom\s+"(?:\.\.\/)+(?:app|client|bindings|ui|theme)\//.test(source)) {
        escapes.push(relative(SRC, full));
      }
    }
  };
  walk(join(SRC, "kit"));
  return escapes;
}

test("a surface never spells a kernel-only token", () => {
  const failures = productionSources().flatMap((path) => {
    const found = violations(path, readFileSync(path, "utf8"));
    return found.length === 0 ? [] : [`${relative(SRC, path)}: ${found.join(", ")}`];
  });
  assert.deepEqual(failures, []);
});

test("the guard catches a forbidden line and leaves a clean one alone", () => {
  const bad = violations(
    join(SRC, "ui", "Fake.tsx"),
    'const height = renderer.terminalHeight;\nconst rule = "─".repeat(3);\nimport { Pane } from "../kit/layout/Pane";',
  );
  assert.deepEqual(bad.sort(), [".repeat(", "a deep kernel import", "terminalHeight"]);
  const clean = violations(
    join(SRC, "ui", "Fake.tsx"),
    'import { Pane, Rule } from "../kit";\nconst ready = true;\n<Rule color="red" />;',
  );
  assert.deepEqual(clean, []);
});

test("a surface draws no box and passes no layout prop", () => {
  assert.deepEqual(jsxViolations("<box height={2} />"), ["<box", "layout prop height"]);
  assert.deepEqual(jsxViolations("<scrollbox flexGrow={1} />"), [
    "<scrollbox",
    "layout prop flexGrow",
  ]);
  assert.deepEqual(jsxViolations('<Line height={1} paddingLeft={2} border="top" />').sort(), [
    "layout prop border",
    "layout prop height",
    "layout prop paddingLeft",
  ]);
  assert.deepEqual(
    jsxViolations(
      "<Pane fill borderColor={ink}><Spacer rows={1} /><Cell text={text} background={ink} /></Pane>",
    ),
    [],
    "the bricks' own props are the surface's vocabulary",
  );
});

test("the kernel imports nothing from the surfaces it serves", () => {
  assert.deepEqual(kernelEscapes(), []);
});
