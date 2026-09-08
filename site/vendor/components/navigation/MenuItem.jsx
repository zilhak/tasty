import React from "react";

/**
 * Tasty MenuItem — a row in a context menu, command palette, or tools
 * menu. 28px tall, icon + label + optional trailing shortcut. Supports
 * a danger treatment and a separator variant.
 */

const CSS = `
.tasty-menuitem {
  display: flex;
  align-items: center;
  gap: var(--tasty-space-sm);
  height: var(--tasty-control-height);
  padding: 0 var(--tasty-space-md);
  border-radius: var(--tasty-radius-sm);
  color: var(--tasty-text-primary);
  font-family: var(--tasty-font-ui);
  font-size: var(--tasty-font-size-body);
  cursor: pointer;
  user-select: none;
  white-space: nowrap;
  transition: background var(--tasty-motion-ui-fast) var(--tasty-ease-ui);
}
.tasty-menuitem:hover { background: var(--tasty-overlay-hover); }
.tasty-menuitem--active { background: var(--tasty-surface-active); }
.tasty-menuitem--danger { color: var(--tasty-accent-danger); }
.tasty-menuitem__icon { flex: none; display: inline-flex; color: var(--tasty-text-muted); }
.tasty-menuitem--danger .tasty-menuitem__icon { color: var(--tasty-accent-danger); }
.tasty-menuitem__icon svg { width: var(--tasty-icon-size-md); height: var(--tasty-icon-size-md); display: block; }
.tasty-menuitem__label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
.tasty-menuitem__shortcut { flex: none; font-family: var(--tasty-font-mono); font-size: var(--tasty-menu-item-shortcut-font-size); color: var(--tasty-text-muted); }
.tasty-menuitem[data-disabled="true"] { opacity: var(--tasty-state-disabled-opacity); pointer-events: none; }
.tasty-menu-sep { height: var(--tasty-border-width); margin: var(--tasty-space-xs) 0; background: var(--tasty-separator); }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-menuitem-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export function MenuItem({
  label,
  icon = null,
  shortcut = null,
  danger = false,
  active = false,
  disabled = false,
  separator = false,
  className = "",
  ...rest
}) {
  ensureCss();
  if (separator) return <div className="tasty-menu-sep" role="separator" />;
  const cls = [
    "tasty-menuitem",
    danger ? "tasty-menuitem--danger" : "",
    active ? "tasty-menuitem--active" : "",
    className,
  ].filter(Boolean).join(" ");
  return (
    <div className={cls} data-disabled={disabled} role="menuitem" {...rest}>
      {icon && <span className="tasty-menuitem__icon">{icon}</span>}
      <span className="tasty-menuitem__label">{label}</span>
      {shortcut != null && <span className="tasty-menuitem__shortcut">{shortcut}</span>}
    </div>
  );
}
