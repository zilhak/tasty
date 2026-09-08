import * as React from "react";

export interface MultiSelectOption {
  value: string;
  label: string;
  /** Row is visible but not togglable (state absent from the data, no permission). */
  disabled?: boolean;
}

export interface MultiSelectProps extends Omit<React.HTMLAttributes<HTMLDivElement>, "onChange"> {
  /** Options as strings or {value,label,disabled}. */
  options?: (string | MultiSelectOption)[];
  /** Controlled selection. Omit for uncontrolled (`defaultValue`). */
  value?: string[];
  defaultValue?: string[];
  onChange?: (next: string[]) => void;
  /**
   * Trigger summary — the widget owns no translations, the caller injects the
   * string. Return null for "0 selected" to fall back to `placeholder`.
   * Default (English): null / "3 selected" / "All".
   */
  summary?: (count: number, total: number) => string | null;
  /** 0-selected label, rendered in placeholder tone. */
  placeholder?: string;
  /** Top "Select all / Clear all" action row + separator. Off by default. */
  allToggle?: boolean;
  allLabel?: string;
  clearLabel?: string;
  /** Stretch to fill the container width. */
  block?: boolean;
  disabled?: boolean;
  /** px cap for the option list; it scrolls past this (default 220). */
  maxMenuHeight?: number | string;
  /** Controlled open state (specimens/tests). */
  open?: boolean;
  /** Controlled keyboard-active row index (specimens/tests). */
  activeIndex?: number;
  /** Force a row's pointer-hover wash (specimens/tests). */
  hoverIndex?: number | null;
}

/**
 * Select-shaped trigger over a Checkbox-row menu that stays open while you
 * toggle. Sibling of Select, not a variant of it.
 */
export function MultiSelect(props: MultiSelectProps): JSX.Element;
