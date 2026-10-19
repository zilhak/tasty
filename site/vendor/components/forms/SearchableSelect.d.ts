import * as React from "react";

export interface SearchableSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface SearchableSelectProps extends Omit<React.HTMLAttributes<HTMLDivElement>, "onChange"> {
  /** Options as strings or {value,label,disabled}. Filtered by the search field. */
  options?: (string | SearchableSelectOption)[];
  /** Always-shown rows above the list (ignore the query), e.g. "Default (monospace)". */
  pinned?: (string | SearchableSelectOption)[];
  value?: string | null;
  defaultValue?: string | null;
  onChange?: (next: string) => void;
  /** Trigger label when nothing is selected (placeholder tone). */
  placeholder?: string;
  /** i18n strings — the widget owns no translations. */
  searchPlaceholder?: string;
  emptyLabel?: string;
  loadingLabel?: string;
  /** Options are still arriving (async font list): shows a Spinner status row at the list end. */
  loading?: boolean;
  block?: boolean;
  disabled?: boolean;
  /** Specimen/test hooks. */
  open?: boolean;
  query?: string;
  activeIndex?: number;
  hoverIndex?: number | null;
  focusSearch?: boolean;
  hoverTrigger?: boolean;
}

/** Case-insensitive substring filter on label. */
export function filterOptions(items: SearchableSelectOption[], query: string): SearchableSelectOption[];

/**
 * Select trigger over a menu popover with a search field on top. Closed = Select.
 */
export function SearchableSelect(props: SearchableSelectProps): JSX.Element;
