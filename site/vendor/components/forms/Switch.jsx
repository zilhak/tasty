import React from "react";

/**
 * Tasty Switch — a compact toggle for boolean settings (notifications,
 * reduced motion, performance flags). Accent-filled when on.
 */

const CSS = `
.tasty-switch { display: inline-flex; align-items: center; gap: var(--tasty-space-sm); cursor: pointer; user-select: none; }
.tasty-switch input { position: absolute; opacity: 0; width: 0; height: 0; }
.tasty-switch__track {
  position: relative; width: var(--tasty-switch-track-width); height: var(--tasty-switch-track-height); flex: none;
  border-radius: var(--tasty-radius-pill);
  background: var(--tasty-surface-active);
  border: var(--tasty-border-width) solid var(--tasty-border-default);
  transition: background var(--tasty-motion-ui) var(--tasty-ease-ui);
}
.tasty-switch__thumb {
  position: absolute; top: var(--tasty-switch-thumb-inset); left: var(--tasty-switch-thumb-inset);
  width: var(--tasty-switch-thumb-size); height: var(--tasty-switch-thumb-size); border-radius: var(--tasty-radius-pill);
  background: var(--tasty-switch-thumb-bg);
  transition: transform var(--tasty-motion-ui) var(--tasty-ease-ui), background var(--tasty-motion-ui) var(--tasty-ease-ui);
}
.tasty-switch input:checked + .tasty-switch__track { background: var(--tasty-accent-primary); border-color: var(--tasty-accent-primary); }
.tasty-switch input:checked + .tasty-switch__track .tasty-switch__thumb { transform: translateX(var(--tasty-switch-thumb-travel)); background: var(--tasty-switch-thumb-bg-on); }
.tasty-switch input:focus-visible + .tasty-switch__track { outline: var(--tasty-focus-ring-width) solid var(--tasty-border-focus); outline-offset: 2px; }
.tasty-switch__label { font-size: var(--tasty-font-size-body); color: var(--tasty-text-primary); }
.tasty-switch[data-disabled="true"] { opacity: var(--tasty-state-disabled-opacity); pointer-events: none; }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-switch-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export function Switch({ label, checked, defaultChecked, disabled = false, className = "", ...rest }) {
  ensureCss();
  return (
    <label className={["tasty-switch", className].filter(Boolean).join(" ")} data-disabled={disabled}>
      <input type="checkbox" role="switch" checked={checked} defaultChecked={defaultChecked} disabled={disabled} {...rest} />
      <span className="tasty-switch__track"><span className="tasty-switch__thumb" /></span>
      {label != null && <span className="tasty-switch__label">{label}</span>}
    </label>
  );
}
