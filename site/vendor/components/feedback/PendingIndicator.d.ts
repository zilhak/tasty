import * as React from "react";

export interface PendingIndicatorProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Pending requests in THIS workspace. 0 → nothing drawn. */
  count?: number;
  /** Seconds the OLDEST pending request has waited (caller-owned clock). */
  elapsed?: number;
  /** i18n override of the label ("Waiting for response" / "Waiting for N responses"). */
  label?: string;
  /** i18n override of the formatted time. */
  time?: string;
  /** Narrow window: spinner + time only (label moves to aria-label / tooltip). */
  compact?: boolean;
  /** Force the still arc (reduced motion, hidden window). */
  reducedMotion?: boolean;
  /** Pending request kinds, one per line ("Close tab · 1m 04s"). Never internal ids. */
  tooltip?: string;
}

/** 12s · 1m 04s · 1h 02m */
export function formatElapsed(sec: number, units?: { s: string; m: string; h: string; sep: string }): string;
export function pendingLabel(count: number): string;

/** View-only "waiting for a response" chip for the window's top-right notice column. */
export function PendingIndicator(props: PendingIndicatorProps): JSX.Element | null;
