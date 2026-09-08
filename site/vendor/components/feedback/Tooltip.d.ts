import * as React from "react";

export interface TooltipProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Bubble content — short, terse copy (1–3 sentences). */
  content?: React.ReactNode;
  /** Bubble position relative to the anchor. */
  placement?: "top" | "bottom" | "left" | "right";
  /** Force the bubble visible (for specimens / always-on demos). */
  open?: boolean;
  /** The anchor the tooltip explains (a HelpHint, label, or icon). */
  children?: React.ReactNode;
}

/**
 * Hover/focus bubble that explains its anchor. Opaque raised card, 1px edge,
 * popover lift, no arrow. Shows on anchor hover or keyboard focus-within after
 * a short delay; minimal fade (suppressed under reduced-motion).
 */
export function Tooltip(props: TooltipProps): JSX.Element;
