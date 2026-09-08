import * as React from "react";

export interface AutoCompleteItem {
  value: string;
  label: string;
  /** Optional per-row icon; falls back to the component `rowIcon`. */
  icon?: React.ReactNode;
}

export interface AutoCompleteProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, "onSelect"> {
  /** Leading icon on the trigger field (per-surface: folderOpen / file). */
  icon?: React.ReactNode;
  /** Icon on every candidate row (per-surface injection). */
  rowIcon?: React.ReactNode;
  /** Candidate list — strings or {value,label,icon}. */
  items?: (string | AutoCompleteItem)[];
  /** Monospace path variant — path candidates render middle-ellipsis. */
  mono?: boolean;
  /** Stretch to fill the container width. */
  block?: boolean;
  /** Trailing Go (arrow-right) button — the address-bar affordance. */
  withGo?: boolean;
  /** Filter mode. Default "substring" (path-friendly). */
  match?: "substring" | "prefix" | "none";
  /** Highlight the matched run in each row. Default true. */
  highlight?: boolean;
  /** Max dropdown height in px (number) or CSS length; list scrolls past it,
   *  shrinks-to-fit below. Defaults to `--tasty-autocomplete-max-height`. */
  maxDropdownHeight?: number | string;
  /** Muted row shown when there is no match / no candidates. */
  emptyLabel?: string;
  /** Controlled open state (specimens/tests). */
  open?: boolean;
  /** Controlled keyboard-active row index (specimens/tests). */
  activeIndex?: number;
  /** Controlled filter/query text (specimens/tests). */
  query?: string;
  /** Force a row's pointer-hover wash by index (specimens). */
  hoverIndex?: number | null;
  onSelect?: (value: string) => void;
  onGo?: (value: string) => void;
}

/**
 * Free-text trigger (Input language) + floating candidate dropdown
 * (menu-container + MenuItem language). A real typeahead — typing narrows the
 * list — not a closed picker (that is Select). Backs both address bars.
 */
export function AutoComplete(props: AutoCompleteProps): JSX.Element;
