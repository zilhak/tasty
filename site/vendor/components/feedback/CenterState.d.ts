import * as React from "react";

export interface CenterStateProps extends React.HTMLAttributes<HTMLDivElement> {
  /** loading = Spinner · empty = host glyph · error = alertTriangle (part-owned). Default "empty". */
  variant?: "loading" | "empty" | "error";
  /** Icon name for the EMPTY variant only. Ignored for loading / error. Default "folderOpen". */
  glyph?: string;
  /** Title line — body 13 · text-secondary. */
  title: React.ReactNode;
  /** Sub line — caption 11 · text-muted. The slot is always reserved, even when empty. */
  sub?: React.ReactNode;
  /** Optional action (Button variant="secondary" size="sm"). Hangs below the block, outside the centring. */
  action?: React.ReactNode;
}

/** The one centred empty / loading / error block that replaces a list region. */
export function CenterState(props: CenterStateProps): JSX.Element;
export const CENTER_STATE_ERROR_GLYPH: "alertTriangle";
