import * as React from "react";

export interface CodeAreaProps extends Omit<React.TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "defaultValue"> {
  /** Controlled text. */
  value?: string;
  /** Uncontrolled initial text. */
  defaultValue?: string;
  /** Minimum visible rows (gutter shows at least this many numbers). Default 4. */
  minRows?: number;
  /** Danger border. Implied when errorLine is set. */
  invalid?: boolean;
  /** 1-based line to mark: danger gutter number + tinted band. */
  errorLine?: number | null;
  /** Neutral disabled box + disabled ink. */
  disabled?: boolean;
}

/**
 * Multi-line monospace field with a line-number gutter. Same box as Input; no wrap;
 * grows to --tasty-codearea-max-height then scrolls.
 */
export function CodeArea(props: CodeAreaProps): JSX.Element;
