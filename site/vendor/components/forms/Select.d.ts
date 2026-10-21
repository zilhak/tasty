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
  /** 2026-10-19 — text shown in the trigger (--tasty-select-placeholder ink) while the value is "".
   *  Rendered as a hidden, disabled "" option: never a row in the open list. Pair with value="" / no defaultValue. */
  placeholder?: string;
  children?: React.ReactNode;
}

/**
 * Native-backed dropdown styled to match Input, with a custom chevron.
 */
export function Select(props: SelectProps): JSX.Element;
