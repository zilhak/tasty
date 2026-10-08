import * as React from "react";

export interface MenuItemProps extends React.HTMLAttributes<HTMLDivElement> {
  /** Row label. */
  label?: React.ReactNode;
  /** Leading icon. */
  icon?: React.ReactNode;
  /** Trailing shortcut text (e.g. "Ctrl+T"). */
  shortcut?: React.ReactNode;
  /** Destructive treatment (red). */
  danger?: boolean;
  /** Highlighted (keyboard-focused) row. */
  active?: boolean;
  /** Selected option — the current value of a Select-style list. Text-primary ink + trailing accent check; no fill. */
  selected?: boolean;
  /** Wrap the label instead of ellipsising — for fixed copy that is wider than the menu's max width. The row grows past 28px. */
  wrap?: boolean;
  /** Disabled row. */
  disabled?: boolean;
  /** Render a thin separator instead of a row. */
  separator?: boolean;
}

/**
 * A 28px row for context menus, the command palette, and tools menus —
 * icon + label + trailing shortcut, with danger and separator variants.
 */
export function MenuItem(props: MenuItemProps): JSX.Element;
