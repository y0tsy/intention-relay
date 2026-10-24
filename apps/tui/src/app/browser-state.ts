//! What the session browser shows: its list, its tab, and its filter.
//!
//! The tab and the filter choose the rows; the highlight lives with the
//! surface that owns the keyboard, so the browser never cursors a row the
//! surface cannot see.

import type { SessionSummariesDto, SessionSummaryDto } from "../bindings";
import { BROWSER_TABS, browserRows, type BrowserTab } from "./browser";

/** Owns the browser's list, tab, filter, and cursor. */
export class BrowserState {
  /** The session list the browser shows, once the daemon has answered. */
  summaries: SessionSummariesDto | null = null;
  /** The failure the session browser reports instead of a list. */
  failure: string | null = null;
  /** The browser's tab. */
  tab: BrowserTab = "all";
  /** The word the browser filters its rows by. */
  filter = "";

  /** Returns the rows the browser's tab and filter show. */
  rows(workspaceRoot: string): readonly SessionSummaryDto[] {
    return browserRows(this.summaries?.sessions ?? [], this.tab, this.filter, workspaceRoot);
  }

  /** Records one fresh list. */
  listed(summaries: SessionSummariesDto): void {
    this.summaries = summaries;
    this.failure = null;
  }

  /** Records one failed list: the list is gone, the failure is the line. */
  failed(failure: string): void {
    this.summaries = null;
    this.failure = failure;
  }

  /** Records the request in flight: no list and no failure. */
  requested(): void {
    this.summaries = null;
    this.failure = null;
  }

  /** Switches the tab. */
  moveTab(delta: number): void {
    const index = BROWSER_TABS.indexOf(this.tab);
    const count = BROWSER_TABS.length;
    this.tab = BROWSER_TABS[(((index + delta) % count) + count) % count] ?? "all";
  }

  /** Types one character into the filter. */
  typeFilter(text: string): void {
    this.filter += text;
  }

  /** Deletes the last character of the filter. */
  backspaceFilter(): void {
    this.filter = [...this.filter].slice(0, -1).join("");
  }
}
