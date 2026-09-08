import * as React from "react";

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectProps extends React.SelectHTMLAttributes<HTMLSelectElement> {
  /** Options as strings or {value,label}. Ignored if children are passed. */
  options?: (string | SelectOption)[];
  /** Stretch to fill the container width. */
  block?: boolean;
  children?: React.ReactNode;
}

/**
 * Native-backed dropdown styled to match Input, with a custom chevron.
 */
export function Select(props: SelectProps): JSX.Element;
