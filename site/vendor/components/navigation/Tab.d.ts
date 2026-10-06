import * as React from "react";

export interface TabProps extends React.HTMLAttributes<HTMLDivElement> {
  /** Tab label (truncates with ellipsis). */
  label: React.ReactNode;
  /** Optional leading icon (surface kind). */
  icon?: React.ReactNode;
  /** Active/focused tab — adopts panel color + accent top bar. */
  active?: boolean;
  /**
   * Live-activity dot, MIRRORING the product surface model: the body exposes
   * one activity bool per surface, so the tab shows one dot — busy=green,
   * idle=no dot. The 5-color owner×activity vocabulary (running/waiting/agent/
   * error) has no per-surface data source and lives only on the WORKSPACE
   * StatusDot, never here. Tasty has no "unsaved/dirty" state — this replaces it.
   */
  status?: "idle" | "busy";
  /**
   * Attention kind — tints the title (tab-fg-needs-input / tab-fg-completion).
   * Ignored on the active tab (attention clears on focus). Replaces the removed
   * `attached` / `notif` props (2026-10-07) — the product tab draws neither.
   */
  attention?: "needs-input" | "completion" | null;
  /** Fired when the close affordance is clicked. */
  onClose?: (e: React.MouseEvent) => void;
}

/**
 * One tab in the tab strip. 24px tall, 150px wide, with a hover-revealed
 * close button and an accent bar on the active tab.
 */
export function Tab(props: TabProps): JSX.Element;
