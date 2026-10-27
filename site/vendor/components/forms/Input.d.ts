import * as React from "react";

/**
 * @startingPoint section="Forms" subtitle="Text field with icon, addon, states" viewport="700x280"
 */
export interface InputProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "size"> {
  /** Leading icon element inside the field. */
  icon?: React.ReactNode;
  /** Trailing text addon (e.g. a unit or count). */
  addon?: React.ReactNode;
  /** Stretch to fill the container width. */
  block?: boolean;
  /** Use the monospace family (paths, IDs, regex). */
  mono?: boolean;
  /** Error state — red border in every state; focused adds the red ring (2026-10-27). */
  invalid?: boolean;
  /** Unavailable control — neutral box + disabled ink (text-disabled), no pointer, not selectable. */
  disabled?: boolean;
  /** Not editable now, value still read (2026-09-29). Neutral box like disabled, value in
   *  --tasty-input-readonly-fg (text-secondary-raised). Focusable, selectable, copyable. Ignored when disabled. */
  readOnly?: boolean;
  /** Ground the field sits on (2026-10-14). "panel" (default) = bg-panel or lower (dialogs, panels).
   *  "raised" = surface-raised (menus, banners, popovers): box → input-raised-* / state-disabled-*-raised
   *  (bg-panel + border-strong) in every state, so it never merges with the ground.
   *  Command palette and terminal search bar fields are raised consumers (2026-10-27), not exceptions. */
  ground?: "panel" | "raised";
  /** Paint the focused box without DOM focus — static gallery specimens only; live fields use real focus. */
  focused?: boolean;
}

/**
 * Single-line text field. 28px tall, 1px border, accent focus ring.
 */
export function Input(props: InputProps): JSX.Element;
