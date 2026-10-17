import * as React from "react";

export interface LoadingOverlayRect { x: number | string; y: number | string; width: number | string; height: number | string; }

export interface LoadingOverlayProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Show or hide. The caller owns the request lifetime and the loading-overlay-delay. Default true. */
  visible?: boolean;
  /** Placement: surface (corner bed) · tab (kind-icon slot) · lnb (workspace dot slot) · rail (ring around the avatar initial). */
  kind?: "surface" | "tab" | "lnb" | "rail";
  /** Target rect relative to the positioned parent. Omit to fill the parent (inset 0). */
  rect?: LoadingOverlayRect;
  /** tab / lnb only: the slot bed = the cell's or row's current resolved fill, so the glyph under it is covered. */
  bed?: string;
  /** Accessible label (translation key value). Default "Loading". */
  label?: string;
  /** Force the still arc. The OS reduced-motion setting also shows the still arc. */
  reducedMotion?: boolean;
}

/**
 * Spinner laid over existing content while a slow request runs. Adds no slot, changes no geometry,
 * takes no input (pointer-events none), owns no timers.
 */
export function LoadingOverlay(props: LoadingOverlayProps): JSX.Element | null;
