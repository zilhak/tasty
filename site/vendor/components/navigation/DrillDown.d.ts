import * as React from "react";

export interface DrillDownProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, "title"> {
  /** Which view is shown. Controlled by the caller. */
  view?: "list" | "detail";
  /** Detail-view title, shown in the back bar next to the ← button. */
  title?: React.ReactNode;
  /** Back-button handler (returns to the list view). */
  onBack?: () => void;
  /** Right-aligned back-bar actions — the canonical home for a detail
   *  action like "Apply", kept clear of the surrounding modal footer. */
  actions?: React.ReactNode;
  /** Detail-view body (shown when `view === "detail"`). Scrolls internally. */
  detail?: React.ReactNode;
  /** Accessible label / tooltip for the back button. Default "Back". */
  backLabel?: string;
  /** Opt-in cross-fade on view change (skipped under reduced-motion).
   *  Default false — instant swap. */
  animate?: boolean;
  /** List-view content (shown when `view === "list"`) — typically a ListCtrl. */
  children?: React.ReactNode;
}

/**
 * Generic master → detail CONTENT-SWAP layout: a full-width list view that
 * swaps to a full-width detail view (no side-by-side split). Detail carries a
 * pinned back bar (← + title + optional right-aligned actions). Instant switch
 * by default. Pair with ListCtrl. Fills its container; detail body scrolls.
 */
export function DrillDown(props: DrillDownProps): JSX.Element;
