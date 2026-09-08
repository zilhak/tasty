import React from "react";

/**
 * Tasty Badge — a compact count or status pill. Used for unread
 * notification counts ("99+") and small numeric indicators.
 *
 * Variants are bg/fg token PAIRS (--tasty-badge-<variant>-bg/-fg). `primary`
 * (blue) = Completion attention, `warning` (yellow) = NeedsInput attention.
 * When a row shows both, wrap them in <BadgeGroup> so the gap is tokenized and
 * order is fixed: NeedsInput leads, Completion trails.
 */

const CSS = `
.tasty-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: var(--tasty-badge-size);
  height: var(--tasty-badge-size);
  padding: 0 var(--tasty-badge-padding-x);
  border-radius: var(--tasty-radius-pill);
  font-family: var(--tasty-font-mono);
  font-size: var(--tasty-badge-font-size);
  font-weight: var(--tasty-font-weight-bold);
  line-height: var(--tasty-line-height-tight);
  color: var(--tasty-badge-danger-fg);
  background: var(--tasty-badge-danger-bg);
}
.tasty-badge--primary { background: var(--tasty-badge-primary-bg); color: var(--tasty-badge-primary-fg); }
.tasty-badge--warning { background: var(--tasty-badge-warning-bg); color: var(--tasty-badge-warning-fg); }
.tasty-badge--agent   { background: var(--tasty-badge-agent-bg);   color: var(--tasty-badge-agent-fg); }
.tasty-badge--success { background: var(--tasty-badge-success-bg); color: var(--tasty-badge-success-fg); }
.tasty-badge--neutral { background: var(--tasty-badge-neutral-bg); color: var(--tasty-badge-neutral-fg); }
.tasty-badge-group { display: inline-flex; align-items: center; gap: var(--tasty-badge-group-gap); }
.tasty-badge--dot { min-width: var(--tasty-badge-dot-size); width: var(--tasty-badge-dot-size); height: var(--tasty-badge-dot-size); padding: 0; }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-badge-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export function Badge({ variant = "danger", dot = false, className = "", children, ...rest }) {
  ensureCss();
  const cls = [
    "tasty-badge",
    variant !== "danger" ? `tasty-badge--${variant}` : "",
    dot ? "tasty-badge--dot" : "",
    className,
  ].filter(Boolean).join(" ");
  return (
    <span className={cls} {...rest}>
      {!dot && children}
    </span>
  );
}

export function BadgeGroup({ className = "", children, ...rest }) {
  ensureCss();
  return (
    <span className={["tasty-badge-group", className].filter(Boolean).join(" ")} {...rest}>
      {children}
    </span>
  );
}
