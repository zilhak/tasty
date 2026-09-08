import * as React from "react";

export interface SpinnerProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Diameter in px (or any CSS length string). Default 16. */
  size?: number | string;
  /** Stroke width of the ring, in viewBox units. Default 2. */
  stroke?: number;
  /** Accessible label announced to screen readers. Default "Loading". */
  label?: React.ReactNode;
}

/**
 * Small indeterminate progress indicator for short background work.
 * A thin rotating arc on a faint track; falls back to three static dots
 * under prefers-reduced-motion.
 */
export function Spinner(props: SpinnerProps): JSX.Element;
