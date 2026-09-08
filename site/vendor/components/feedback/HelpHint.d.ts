import * as React from "react";

export interface HelpHintProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Tooltip explanation shown on hover. Omit for a bare (?) glyph. */
  label?: React.ReactNode;
  /** Tooltip placement relative to the glyph. */
  placement?: "top" | "bottom" | "left" | "right";
}

/**
 * Inline circular "?" glyph after a label, hover-only. Muted at rest,
 * brightens on hover/focus; anchors a Tooltip with the explanation.
 */
export function HelpHint(props: HelpHintProps): JSX.Element;
