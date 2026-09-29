import React from "react";
import { Icon } from "../core/Icon";
import { Spinner } from "./Spinner";

/**
 * Tasty CenterState — the one centred empty / loading / error block that
 * replaces a list region (file picker, remote attach, Settings › Misc › Scripts).
 *
 * Owned by the part (hosts cannot override): sizes, colours, type, and the
 * ERROR glyph (alertTriangle). Hosts pass the empty-state glyph, the copy and
 * an optional action.
 *
 * Centring: only [glyph · title · sub slot] is centred in the region. The
 * optional action hangs BELOW that block (absolute, center-state-action-gap)
 * and is left out of the centring, so loading → empty → error (± action)
 * never moves the glyph. The sub slot is always reserved (one caption line).
 *
 * Unsized hosts (`natural`, 2026-09-29): the host gives no height, so the part
 * sizes itself. With an action it reserves the action's band on BOTH sides —
 * padding-block = space-md + center-state-action-gap + button-height-sm (48) —
 * so the block stays centred, the action ends space-md above the bottom edge and
 * nothing overflows. Natural height = block + 2 × 48; without an action = block + 2 × 12.
 */

const CSS = `
.tasty-cstate { flex: 1; min-height: 0; display: flex; align-items: center; justify-content: center; padding: var(--tasty-space-md); }
.tasty-cstate--natural { flex: none; }
.tasty-cstate--natural[data-action="true"] { padding-block: calc(var(--tasty-space-md) + var(--tasty-center-state-action-gap) + var(--tasty-button-height-sm)); }
.tasty-cstate__block { position: relative; display: flex; flex-direction: column; align-items: center; text-align: center; max-width: var(--tasty-center-state-max-width); }
.tasty-cstate__glyph { display: inline-flex; margin-bottom: var(--tasty-center-state-gap); color: var(--tasty-center-state-glyph-fg); }
.tasty-cstate[data-variant="error"] .tasty-cstate__glyph { color: var(--tasty-center-state-error-fg); }
.tasty-cstate__title { font-size: var(--tasty-font-size-body); line-height: var(--tasty-line-height-ui); color: var(--tasty-center-state-title-fg); }
.tasty-cstate__sub { margin-top: var(--tasty-center-state-line-gap); min-height: calc(var(--tasty-font-size-caption) * var(--tasty-line-height-ui));
  font-size: var(--tasty-font-size-caption); line-height: var(--tasty-line-height-ui); color: var(--tasty-center-state-sub-fg); text-wrap: pretty; }
.tasty-cstate__action { position: absolute; top: calc(100% + var(--tasty-center-state-action-gap)); left: 50%; transform: translateX(-50%); display: flex; white-space: nowrap; }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-cstate-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export const CENTER_STATE_ERROR_GLYPH = "alertTriangle";

export function CenterState({ variant = "empty", glyph = "folderOpen", title, sub = null, action = null, natural = false, className = "", ...rest }) {
  ensureCss();
  const g = variant === "error" ? CENTER_STATE_ERROR_GLYPH : glyph;
  return (
    <div className={["tasty-cstate", natural ? "tasty-cstate--natural" : "", className].filter(Boolean).join(" ")} data-variant={variant} data-action={!!action} {...rest}>
      <div className="tasty-cstate__block">
        <span className="tasty-cstate__glyph">
          {variant === "loading" ? <Spinner size="var(--tasty-center-state-glyph-size)" /> : <Icon name={g} size="var(--tasty-center-state-glyph-size)" />}
        </span>
        <span className="tasty-cstate__title">{title}</span>
        <span className="tasty-cstate__sub">{sub}</span>
        {action && <span className="tasty-cstate__action">{action}</span>}
      </div>
    </div>
  );
}
