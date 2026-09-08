import React from "react";

/**
 * Tasty Input — single-line text field. 28px tall, 1px border,
 * focus ring in accent-primary. Optional leading icon + addon.
 */

const CSS = `
.tasty-input {
  display: inline-flex;
  align-items: center;
  gap: var(--tasty-space-sm);
  height: var(--tasty-control-height);
  padding: 0 var(--tasty-space-md);
  border: var(--tasty-border-width) solid var(--tasty-border-default);
  border-radius: var(--tasty-radius);
  background: var(--tasty-surface-raised);
  color: var(--tasty-text-primary);
  font-family: var(--tasty-font-ui);
  font-size: var(--tasty-font-size-body);
  transition: border-color var(--tasty-motion-ui-fast) var(--tasty-ease-ui),
              box-shadow var(--tasty-motion-ui-fast) var(--tasty-ease-ui);
}
.tasty-input--block { display: flex; width: 100%; }
.tasty-input--mono input { font-family: var(--tasty-font-mono); font-size: var(--tasty-font-size-caption); }
.tasty-input:focus-within { border-color: var(--tasty-border-focus); box-shadow: 0 0 0 var(--tasty-border-width) var(--tasty-border-focus); }
.tasty-input--invalid { border-color: var(--tasty-accent-danger); }
.tasty-input--invalid:focus-within { box-shadow: 0 0 0 var(--tasty-border-width) var(--tasty-accent-danger); }
.tasty-input input {
  flex: 1; min-width: 0;
  border: 0; outline: 0; padding: 0; margin: 0;
  background: transparent; color: inherit; font: inherit;
}
.tasty-input input::placeholder { color: var(--tasty-text-placeholder); }
.tasty-input input:disabled { cursor: not-allowed; }
.tasty-input[data-disabled="true"] { opacity: var(--tasty-state-disabled-opacity); pointer-events: none; }
.tasty-input__icon { display: inline-flex; color: var(--tasty-text-muted); flex: none; }
.tasty-input__icon svg { width: var(--tasty-icon-size-md); height: var(--tasty-icon-size-md); display: block; }
.tasty-input__addon { color: var(--tasty-text-muted); font-family: var(--tasty-font-mono); font-size: var(--tasty-font-size-caption); flex: none; }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-input-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export function Input({
  icon = null,
  addon = null,
  block = false,
  mono = false,
  invalid = false,
  disabled = false,
  className = "",
  ...rest
}) {
  ensureCss();
  const cls = [
    "tasty-input",
    block ? "tasty-input--block" : "",
    mono ? "tasty-input--mono" : "",
    invalid ? "tasty-input--invalid" : "",
    className,
  ].filter(Boolean).join(" ");
  return (
    <div className={cls} data-disabled={disabled}>
      {icon && <span className="tasty-input__icon">{icon}</span>}
      <input disabled={disabled} {...rest} />
      {addon && <span className="tasty-input__addon">{addon}</span>}
    </div>
  );
}
