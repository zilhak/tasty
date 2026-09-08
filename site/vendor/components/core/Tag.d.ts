import * as React from "react";

export interface TagProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Color treatment. default = outlined neutral. */
  variant?: "default" | "accent" | "agent" | "success" | "warning" | "attention" | "danger" | "info";
  /** Prefix with a small status dot in the current color. */
  dot?: boolean;
  children?: React.ReactNode;
}

/**
 * Small monospace label for surface kinds, permissions, plugin origins
 * ("built-in"), and detector rule types.
 */
export function Tag(props: TagProps): JSX.Element;
