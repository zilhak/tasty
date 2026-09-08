import * as React from "react";

/**
 * @startingPoint section="Core" subtitle="Buttons — primary, secondary, ghost, danger, agent" viewport="700x300"
 */
export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  /** Visual role. primary/agent/danger are filled; secondary is outlined; ghost is bare. */
  variant?: "primary" | "secondary" | "ghost" | "danger" | "agent";
  /** Control height. md = 28px (default), sm = 24px, lg = 32px. */
  size?: "sm" | "md" | "lg";
  /** Stretch to fill the container width. */
  block?: boolean;
  /** Icon element rendered before the label. */
  leadingIcon?: React.ReactNode;
  /** Icon element rendered after the label. */
  trailingIcon?: React.ReactNode;
  children?: React.ReactNode;
}

/**
 * The primary text button. 28px tall, 4px radius, 1px border, with
 * 8%/12% hover/active overlays derived from the active theme.
 */
export function Button(props: ButtonProps): JSX.Element;
