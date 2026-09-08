import React from "react";
import { Icon } from "../core/Icon";
import { IconButton } from "../core/IconButton";

/**
 * Tasty DrillDown — a generic master → detail CONTENT-SWAP layout. Inside a
 * bounded content area it toggles between a full-width LIST view and a
 * full-width DETAIL view (no side-by-side split). Entering detail replaces
 * the whole area; a top BACK BAR (a ghost ← IconButton + the detail title,
 * with an optional right-aligned actions slot) returns to the list. Use it
 * anywhere the model is "one full-width list, select an item, see its detail,
 * go back" — e.g. Settings › Keybindings › Preset (ListCtrl of presets →
 * preview + Apply).
 *
 * Controlled: the caller owns which view is shown via `view`. The switch is
 * INSTANT by default (no motion) — honouring the calm, 0ms-terminal system;
 * an opt-in `animate` cross-fades only when the user allows motion.
 *
 * Fills its container (100% height, flex column); the detail body scrolls
 * inside itself so the back bar stays pinned.
 */

const CSS = `
.tasty-drilldown { display: flex; flex-direction: column; height: 100%; min-height: 0; min-width: 0; }
.tasty-drilldown__view { flex: 1; min-height: 0; display: flex; flex-direction: column; }
.tasty-drilldown__list { flex: 1; min-height: 0; overflow: auto; }
.tasty-drilldown__backbar {
  display: flex; align-items: center; gap: var(--tasty-drilldown-backbar-gap);
  flex: none; min-height: var(--tasty-drilldown-backbar-height);
  padding: var(--tasty-drilldown-backbar-padding-y) var(--tasty-drilldown-backbar-padding-x);
  border-bottom: var(--tasty-border-width) solid var(--tasty-drilldown-backbar-border);
}
.tasty-drilldown__title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  font-size: var(--tasty-drilldown-title-font-size); font-weight: var(--tasty-drilldown-title-font-weight);
  color: var(--tasty-drilldown-title-fg); }
.tasty-drilldown__actions { flex: none; display: inline-flex; align-items: center; gap: var(--tasty-space-sm); }
.tasty-drilldown__body { flex: 1; min-height: 0; overflow: auto; }
@media (prefers-reduced-motion: no-preference) {
  .tasty-drilldown--animate .tasty-drilldown__view { animation: tasty-drilldown-in var(--tasty-motion-ui) var(--tasty-ease-ui); }
}
@keyframes tasty-drilldown-in { from { opacity: 0; } to { opacity: 1; } }
`;

let injected = false;
function ensureCss() {
  if (injected || typeof document === "undefined") return;
  const el = document.createElement("style");
  el.id = "tasty-drilldown-css";
  el.textContent = CSS;
  document.head.appendChild(el);
  injected = true;
}

export function DrillDown({
  view = "list",
  title,
  onBack,
  actions = null,
  detail = null,
  backLabel = "Back",
  animate = false,
  children,
  className = "",
  ...rest
}) {
  ensureCss();
  const isDetail = view === "detail";
  const cls = ["tasty-drilldown", animate ? "tasty-drilldown--animate" : "", className]
    .filter(Boolean).join(" ");

  return (
    <div className={cls} {...rest}>
      {isDetail ? (
        <div className="tasty-drilldown__view" key="detail">
          <div className="tasty-drilldown__backbar">
            <IconButton size="sm" aria-label={backLabel} title={backLabel} onClick={onBack}>
              <Icon name="chevronLeft" />
            </IconButton>
            <span className="tasty-drilldown__title">{title}</span>
            {actions && <span className="tasty-drilldown__actions">{actions}</span>}
          </div>
          <div className="tasty-drilldown__body">{detail}</div>
        </div>
      ) : (
        <div className="tasty-drilldown__view" key="list">
          <div className="tasty-drilldown__list">{children}</div>
        </div>
      )}
    </div>
  );
}
