import * as React from "react";

export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Color role. Defaults to danger (unread counts). */
  variant?: "danger" | "primary" | "warning" | "agent" | "success" | "neutral";
  /** Render as a bare status dot with no label. */
  dot?: boolean;
  children?: React.ReactNode;
}

/**
 * Compact count or status pill — unread badges ("99+"), numeric
 * indicators, or a bare status dot.
 */
export function Badge(props: BadgeProps): JSX.Element;

/** Row of kind badges sharing one trailing slot — tokenized gap. */
export function BadgeGroup(props: React.HTMLAttributes<HTMLSpanElement>): JSX.Element;
