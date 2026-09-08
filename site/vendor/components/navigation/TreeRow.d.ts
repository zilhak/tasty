import * as React from "react";

export interface TreeRowProps extends React.HTMLAttributes<HTMLDivElement> {
  /** Row label. */
  label: React.ReactNode;
  /** Leading icon (folder, file kind, workspace). */
  icon?: React.ReactNode;
  /** Indent depth (each level = 14px). */
  level?: number;
  /** Show a disclosure chevron (folders). */
  expandable?: boolean;
  /** Chevron rotated open state. */
  open?: boolean;
  /** Selected/active row. */
  selected?: boolean;
  /** Trailing meta (count, shortcut, size). */
  meta?: React.ReactNode;
}

/**
 * A 22px row for sidebars and file trees — indent, disclosure chevron,
 * icon, label, trailing meta, and a selected state.
 */
export function TreeRow(props: TreeRowProps): JSX.Element;
