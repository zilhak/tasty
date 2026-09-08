import * as React from "react";

export interface KbdProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Keys as an array (["Ctrl","Shift","T"]) or a "+"-joined string ("Ctrl+T"). */
  keys: string[] | string;
}

/**
 * Renders a keyboard shortcut as individual keycaps joined by "+".
 * Used in settings, menus, and the command palette.
 */
export function Kbd(props: KbdProps): JSX.Element;
