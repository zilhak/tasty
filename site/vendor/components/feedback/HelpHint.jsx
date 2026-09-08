import React from "react";
import { Tooltip } from "./Tooltip";

/**
 * Tasty HelpHint — an inline circular "?" glyph that sits right after a label
 * and explains it on hover. Hover-only (no click); muted at rest, brightens on
 * hover / keyboard focus. Anchors a Tooltip bubble carrying the explanation.
 * Cursor is `help`. Use it to move a below-control description line up next to
 * its label so the row stays one line.
 *
 * <HelpHint label="Swap old scrollback to disk to reduce memory usage." />
 */

const CSS = `
.tasty-help-hint__glyph {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
  width: var(--tasty-help-hint-size);
  height: var(--tasty-help-hint-size);
  color: var(--tasty-help-hint-color);
  cursor: help;
  vertical-align: middle;
  outline: none;
  transition: color var(--tasty-motion-ui-fast) var(--tasty-ease-ui);
}
.tasty-help-hint__glyph svg { width: 100%; height: 100%; display: block; }
.tasty-help-hint:hover .tasty-help-hint__glyph,
.tasty-help-hint__glyph:hover,
.tasty-help-hint__glyph:focus-visible { color: var(--tasty-help-hint-color-hover); }
@media (prefers-reduced-motion: reduce) { .tasty-help-hint__glyph { transition: none; } }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-help-hint-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

const GLYPH = (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="10" />
    <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
    <path d="M12 17h.01" />
  </svg>
);

export function HelpHint({ label, placement = "top", className = "", ...rest }) {
  ensureCss();
  const glyph = (
    <span className="tasty-help-hint__glyph" tabIndex={0} role="img"
      aria-label={typeof label === "string" ? label : "Help"} {...rest}>
      {GLYPH}
    </span>
  );
  if (label == null) return glyph;
  return (
    <Tooltip content={label} placement={placement} className={["tasty-help-hint", className].filter(Boolean).join(" ")}>
      {glyph}
    </Tooltip>
  );
}
