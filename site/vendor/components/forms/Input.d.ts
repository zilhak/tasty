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
  /** Error state — red border + ring. */
  invalid?: boolean;
}

/**
 * Single-line text field. 28px tall, 1px border, accent focus ring.
 */
export function Input(props: InputProps): JSX.Element;
