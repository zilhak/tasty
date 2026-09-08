import React from "react";

/**
 * Tasty Tooltip — a hover/focus bubble that explains its anchor. Opaque
 * surface-raised card, 1px edge, popover lift, no arrow (calm, low-chrome).
 * Wraps any trigger; the bubble shows on hover of the anchor or keyboard
 * focus within it. Motion is minimal — a short fade with a small show delay,
 * suppressed under prefers-reduced-motion. Pass `open` to force it visible
 * (used by specimens / always-on demos).
 *
 * <Tooltip content="Explanation…" placement="top"><HelpHint … /></Tooltip>
 */

const CSS = `
.tasty-tooltip { position: relative; display: inline-flex; }
.tasty-tooltip__bubble {
  position: absolute;
  z-index: 40;
  width: max-content;
  max-width: var(--tasty-tooltip-max-width);
  padding: var(--tasty-tooltip-padding-y) var(--tasty-tooltip-padding-x);
  border: var(--tasty-border-width) solid var(--tasty-tooltip-border);
  border-radius: var(--tasty-tooltip-radius);
  background: var(--tasty-tooltip-bg);
  box-shadow: var(--tasty-tooltip-shadow);
  color: var(--tasty-tooltip-fg);
  font-family: var(--tasty-font-ui);
  font-size: var(--tasty-tooltip-font-size);
  line-height: var(--tasty-tooltip-line-height);
  text-align: left;
  white-space: normal;
  opacity: 0;
  visibility: hidden;
  pointer-events: none;
  transition: opacity var(--tasty-motion-ui-fast) var(--tasty-ease-ui);
}
.tasty-tooltip:hover > .tasty-tooltip__bubble,
.tasty-tooltip:focus-within > .tasty-tooltip__bubble,
.tasty-tooltip.is-open > .tasty-tooltip__bubble {
  opacity: 1;
  visibility: visible;
  transition-delay: var(--tasty-tooltip-delay);
}
.tasty-tooltip.is-open > .tasty-tooltip__bubble { transition-delay: 0s; }
/* placement */
.tasty-tooltip--top    > .tasty-tooltip__bubble { bottom: 100%; left: 50%; transform: translateX(-50%); margin-bottom: var(--tasty-tooltip-offset); }
.tasty-tooltip--bottom > .tasty-tooltip__bubble { top: 100%;    left: 50%; transform: translateX(-50%); margin-top: var(--tasty-tooltip-offset); }
.tasty-tooltip--left   > .tasty-tooltip__bubble { right: 100%;  top: 50%;  transform: translateY(-50%); margin-right: var(--tasty-tooltip-offset); }
.tasty-tooltip--right  > .tasty-tooltip__bubble { left: 100%;   top: 50%;  transform: translateY(-50%); margin-left: var(--tasty-tooltip-offset); }
@media (prefers-reduced-motion: reduce) {
  .tasty-tooltip__bubble { transition: none; }
}
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-tooltip-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export function Tooltip({ content, placement = "top", open = false, className = "", children, ...rest }) {
  ensureCss();
  const cls = ["tasty-tooltip", `tasty-tooltip--${placement}`, open ? "is-open" : "", className].filter(Boolean).join(" ");
  return (
    <span className={cls} {...rest}>
      {children}
      <span className="tasty-tooltip__bubble" role="tooltip">{content}</span>
    </span>
  );
}
