import * as React from "react";

export interface TableColumn<Row = any> {
  /** Unique key; also the row field read when `render` is omitted. */
  key: string;
  /** Header cell content. */
  header: React.ReactNode;
  /** Cell alignment. Default "left". Use "right" for numbers/ports. */
  align?: "left" | "right" | "center";
  /** Render cell value with --tasty-font-mono (ports, PIDs, addresses, IDs). */
  mono?: boolean;
  /** Render cell in --tasty-text-primary without mono (emphasis column). */
  strong?: boolean;
  /** Clickable sort header (pair with `sort`/`onSort`). */
  sortable?: boolean;
  /** Fixed column width (any CSS length). */
  width?: string;
  /**
   * Zero horizontal padding + centered content — for narrow icon/control
   * columns (a star toggle, a checkbox) where the default 12px cell padding
   * would not leave room for the glyph.
   */
  tight?: boolean;
  /** Custom cell renderer; receives (value, row). */
  render?: (value: any, row: Row) => React.ReactNode;
}

export interface TableSort {
  key: string;
  dir: "asc" | "desc";
}

export interface TableProps<Row = any>
  extends Omit<React.TableHTMLAttributes<HTMLTableElement>, "rows"> {
  /** Column configuration, left to right. */
  columns: TableColumn<Row>[];
  /** Row objects. */
  rows: Row[];
  /** Row identity: a field name or (row) => key. Defaults to index. */
  rowKey?: string | ((row: Row) => React.Key);
  /** Selected row key (renders --tasty-surface-active). */
  selectedKey?: React.Key | null;
  /** Row click handler; also makes rows look clickable. */
  onRowClick?: (row: Row, key: React.Key) => void;
  /** Active sort, shows the header arrow. */
  sort?: TableSort | null;
  /** Sortable-header click handler; receives the column key. */
  onSort?: (key: string) => void;
  /** Compact 22px rows (tree density) instead of 28px. */
  dense?: boolean;
  /** Row hover highlight. Default true. */
  hover?: boolean;
  /** Empty-state content when `rows` is empty. */
  empty?: React.ReactNode;
}

/**
 * Dense, monospace-friendly data table for system data (ports,
 * processes, sessions, keybindings). Column-config driven with
 * per-column alignment + mono, sortable headers, row hover/selection,
 * and a sticky header for scroll regions.
 */
export function Table<Row = any>(props: TableProps<Row>): JSX.Element;
