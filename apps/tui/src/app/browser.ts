//! The sessions browser's model: its tabs, its filter, and the rows it shows.
//!
//! The browser never invents a session: it ranks what the daemon listed. The
//! folder tab keeps the sessions bound to the workspace this front end runs in,
//! the all tab keeps every listed session, and the filter narrows either by the
//! facts a summary carries. A row the filter hides is a row the cursor cannot
//! reach, so the two are computed together.

import type { SessionSummaryDto, WorkspaceRootDto } from "../bindings";

/** The two lists the browser offers. */
export type BrowserTab = "folder" | "all";

/** Every tab, in the order the radio row shows them. */
export const BROWSER_TABS: readonly BrowserTab[] = ["folder", "all"];

/** Returns the label the radio row shows for one tab. */
export function tabLabel(tab: BrowserTab): string {
  return tab === "folder" ? "This workspace" : "All sessions";
}

/**
 * Returns the rows one tab and one filter show.
 *
 * A filter matches the session identity and the workspace path without regard to
 * case; an empty filter matches every row of the tab.
 */
export function browserRows(
  sessions: readonly SessionSummaryDto[],
  tab: BrowserTab,
  filter: string,
  workspaceRoot: WorkspaceRootDto,
): SessionSummaryDto[] {
  const wanted = filter.trim().toLowerCase();
  return sessions.filter((session) => {
    if (tab === "folder" && session.workspace_root !== workspaceRoot) {
      return false;
    }
    if (wanted.length === 0) {
      return true;
    }
    return (
      session.session_id.toLowerCase().includes(wanted) ||
      session.workspace_root.toLowerCase().includes(wanted)
    );
  });
}

/** Returns one durable timestamp as the browser's age columns show it. */
export function relativeTime(seconds: number, now: number): string {
  const age = Math.max(0, now - seconds);
  if (age < 60) {
    return "now";
  }
  if (age < 3_600) {
    return `${Math.floor(age / 60)}m ago`;
  }
  if (age < 86_400) {
    return `${Math.floor(age / 3_600)}h ago`;
  }
  if (age < 604_800) {
    return `${Math.floor(age / 86_400)}d ago`;
  }
  const date = new Date(seconds * 1_000);
  return `${MONTHS[date.getMonth()] ?? "Jan"} ${date.getDate()}`;
}

/** The month labels a date older than a week shows. */
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
