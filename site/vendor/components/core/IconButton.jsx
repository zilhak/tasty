import React from "react";

/**
 * Tasty IconButton — a square, icon-only control (toolbar, tab close,
 * sidebar actions). Matches Button height; ghost by default.
 */

const CSS = `
.tasty-iconbtn {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: var(--tasty-control-height);
  height: var(--tasty-control-height);
  padding: 0;
  border: var(--tasty-border-width) solid transparent;
  border-radius: var(--tasty-radius);
  background: transparent;
  color: var(--tasty-text-secondary);
  cursor: pointer;
  transition: color var(--tasty-motion-ui-fast) var(--tasty-ease-ui),
              background var(--tasty-motion-ui-fast) var(--tasty-ease-ui);
}
.tasty-iconbtn svg { width: var(--tasty-icon-size-md); height: var(--tasty-icon-size-md); display: block; }
.tasty-iconbtn:hover { color: var(--tasty-text-primary); background: var(--tasty-overlay-hover); }
.tasty-iconbtn:active { background: var(--tasty-overlay-active); }
.tasty-iconbtn--sm { width: var(--tasty-control-height-tab); height: var(--tasty-control-height-tab); }
.tasty-iconbtn--sm svg { width: var(--tasty-icon-size-sm); height: var(--tasty-icon-size-sm); }
.tasty-iconbtn--solid { background: var(--tasty-surface-raised); border-color: var(--tasty-border-default); color: var(--tasty-text-primary); }
.tasty-iconbtn--active { color: var(--tasty-accent-primary); background: var(--tasty-overlay-active); }
.tasty-iconbtn[disabled] { opacity: var(--tasty-state-disabled-opacity); pointer-events: none; }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-iconbtn-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export function IconButton({
  size = "md",
  variant = "ghost",
  active = false,
  className = "",
  children,
  ...rest
}) {
  ensureCss();
  const cls = [
    "tasty-iconbtn",
    size === "sm" ? "tasty-iconbtn--sm" : "",
    variant === "solid" ? "tasty-iconbtn--solid" : "",
    active ? "tasty-iconbtn--active" : "",
    className,
  ].filter(Boolean).join(" ");
  return (
    <button className={cls} {...rest}>
      {children}
    </button>
  );
}
