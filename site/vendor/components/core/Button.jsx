import React from "react";

/**
 * Tasty Button — the primary interactive control.
 * 28px tall (item-height-interactive), 1px border, 4px radius,
 * 8%/12% hover/active overlays derived from the active theme.
 */

const CSS = `
.tasty-btn {
  --_h: var(--tasty-control-height);
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--tasty-space-sm);
  height: var(--_h);
  padding: 0 var(--tasty-space-md);
  border: var(--tasty-border-width) solid transparent;
  border-radius: var(--tasty-radius);
  font-family: var(--tasty-font-ui);
  font-size: var(--tasty-font-size-body);
  font-weight: var(--tasty-font-weight-medium);
  line-height: 1;
  white-space: nowrap;
  cursor: pointer;
  user-select: none;
  background: transparent;
  color: var(--tasty-text-primary);
  transition: background var(--tasty-motion-ui-fast) var(--tasty-ease-ui),
              border-color var(--tasty-motion-ui-fast) var(--tasty-ease-ui),
              filter var(--tasty-motion-ui-fast) var(--tasty-ease-ui);
}
.tasty-btn::after {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background: transparent;
  pointer-events: none;
  transition: background var(--tasty-motion-ui-fast) var(--tasty-ease-ui);
}
.tasty-btn:hover::after { background: var(--tasty-overlay-hover); }
.tasty-btn:active::after { background: var(--tasty-overlay-active); }

.tasty-btn--sm { --_h: var(--tasty-button-height-sm); padding: 0 var(--tasty-space-sm); font-size: var(--tasty-font-size-caption); }
.tasty-btn--lg { --_h: var(--tasty-button-height-lg); padding: 0 var(--tasty-space-lg); }

.tasty-btn--primary { background: var(--tasty-accent-primary); color: var(--tasty-text-on-accent); font-weight: var(--tasty-font-weight-semibold); }
.tasty-btn--agent   { background: var(--tasty-accent-agent);   color: var(--tasty-text-on-accent); font-weight: var(--tasty-font-weight-semibold); }
.tasty-btn--danger  { background: var(--tasty-accent-danger);  color: var(--tasty-text-on-accent); font-weight: var(--tasty-font-weight-semibold); }

.tasty-btn--secondary { background: var(--tasty-surface-raised); border-color: var(--tasty-border-default); }
.tasty-btn--secondary:hover { border-color: var(--tasty-border-strong); }

.tasty-btn--ghost { background: transparent; color: var(--tasty-text-secondary); }
.tasty-btn--ghost:hover { color: var(--tasty-text-primary); }

.tasty-btn[disabled] { opacity: var(--tasty-state-disabled-opacity); cursor: not-allowed; pointer-events: none; }
.tasty-btn--block { display: flex; width: 100%; }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-btn-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export function Button({
  variant = "secondary",
  size = "md",
  block = false,
  leadingIcon = null,
  trailingIcon = null,
  className = "",
  children,
  ...rest
}) {
  ensureCss();
  const cls = [
    "tasty-btn",
    `tasty-btn--${variant}`,
    size !== "md" ? `tasty-btn--${size}` : "",
    block ? "tasty-btn--block" : "",
    className,
  ].filter(Boolean).join(" ");
  return (
    <button className={cls} {...rest}>
      {leadingIcon}
      {children != null && <span>{children}</span>}
      {trailingIcon}
    </button>
  );
}
