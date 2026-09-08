import * as React from "react";

export interface ToastProps extends React.HTMLAttributes<HTMLDivElement> {
  /** Accent rail color by intent. */
  variant?: "info" | "success" | "warning" | "danger" | "agent";
  /** Leading status icon. */
  icon?: React.ReactNode;
  /** Trailing monospace hint (e.g. a keybinding or count). */
  hint?: React.ReactNode;
  children?: React.ReactNode;
}

/**
 * Transient notification card with an accent rail and optional icon —
 * "Copied", "Path copied", "Force detach".
 */
export function Toast(props: ToastProps): JSX.Element;
