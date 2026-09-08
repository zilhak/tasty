import * as React from "react";

export interface CheckboxProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "type"> {
  /** Text label rendered next to the box. */
  label?: React.ReactNode;
}

/**
 * 16px square checkbox with accent fill + checkmark when checked.
 */
export function Checkbox(props: CheckboxProps): JSX.Element;
