import React from "react";

/**
 * Tasty Toast — a transient, bottom-anchored notification ("Copied",
 * "Path copied", "Force detach"). Surface-raised card, accent rail,
 * optional icon. Coalesces in the real app; here it's a static cell.
 */

const CSS = `
.tasty-toast {
  display: inline-flex;
  align-items: center;
  gap: var(--tasty-space-sm);
  min-height: var(--tasty-control-height);
  padding: var(--tasty-space-sm) var(--tasty-space-md);
  border: var(--tasty-border-width) solid var(--tasty-border-default);
  border-left-width: var(--tasty-toast-accent-width);
  border-left-color: var(--tasty-accent-primary);
  border-radius: var(--tasty-radius);
  background: var(--tasty-surface-raised);
  color: var(--tasty-text-primary);
  font-family: var(--tasty-font-ui);
  font-size: var(--tasty-font-size-body);
  max-width: var(--tasty-toast-max-width);
}
.tasty-toast--success { border-left-color: var(--tasty-accent-success); }
.tasty-toast--warning { border-left-color: var(--tasty-accent-warning); }
.tasty-toast--danger  { border-left-color: var(--tasty-accent-danger); }
.tasty-toast--agent   { border-left-color: var(--tasty-accent-agent); }
.tasty-toast__icon { flex: none; display: inline-flex; }
.tasty-toast__icon svg { width: var(--tasty-icon-size-md); height: var(--tasty-icon-size-md); display: block; }
.tasty-toast--success .tasty-toast__icon { color: var(--tasty-accent-success); }
.tasty-toast--warning .tasty-toast__icon { color: var(--tasty-accent-warning); }
.tasty-toast--danger  .tasty-toast__icon { color: var(--tasty-accent-danger); }
.tasty-toast--agent   .tasty-toast__icon { color: var(--tasty-accent-agent); }
.tasty-toast--info    .tasty-toast__icon { color: var(--tasty-accent-primary); }
.tasty-toast__msg { flex: 1; }
.tasty-toast__hint { font-family: var(--tasty-font-mono); font-size: var(--tasty-toast-hint-font-size); color: var(--tasty-text-muted); }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-toast-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export function Toast({ variant = "info", icon = null, hint = null, className = "", children, ...rest }) {
  ensureCss();
  const cls = ["tasty-toast", `tasty-toast--${variant}`, className].filter(Boolean).join(" ");
  return (
    <div className={cls} role="status" {...rest}>
      {icon && <span className="tasty-toast__icon">{icon}</span>}
      <span className="tasty-toast__msg">{children}</span>
      {hint != null && <span className="tasty-toast__hint">{hint}</span>}
    </div>
  );
}
