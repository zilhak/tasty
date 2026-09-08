import React from "react";
import { SettingsWindow } from "../../kit/overlays/settings_window.jsx";

/**
 * The settings window, as the app draws it.
 *
 * The kit wraps it in a `Scrim`, which is `position: absolute; inset: 0` — so
 * it fills the box below instead of covering a page, and the same component
 * that renders over the app renders inline here. The window has a fixed design
 * size; the frame zooms it down to the column it has to fit into.
 */
export function SettingsAnatomy() {
  return (
    <div className="anat-settings">
      <div className="anat-settings__stage">
        <SettingsWindow theme="mocha" onTheme={() => {}} uiScale="md" onUiScale={() => {}} onClose={() => {}} />
      </div>
    </div>
  );
}
