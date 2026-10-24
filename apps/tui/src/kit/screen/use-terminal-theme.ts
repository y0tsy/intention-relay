//! The terminal's own background, asked once per preference.
//!
//! A terminal answers an OSC 11 query with its background colour; one that never
//! answers leaves the choice to the caller's fallback.

import { useRenderer } from "@opentui/react";
import { useEffect, useState } from "react";

/** Returns the terminal's theme, or null while it is unknown. */
export function useTerminalTheme(ask: boolean): "light" | "dark" | null {
  const renderer = useRenderer();
  const [terminal, setTerminal] = useState<"light" | "dark" | null>(null);
  useEffect(() => {
    if (!ask) {
      return;
    }
    let alive = true;
    void renderer.waitForThemeMode(400).then((mode) => {
      if (alive && (mode === "light" || mode === "dark")) {
        setTerminal(mode);
      }
    });
    return () => {
      alive = false;
    };
  }, [ask, renderer]);
  return terminal;
}
