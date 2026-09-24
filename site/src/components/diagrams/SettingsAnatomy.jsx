import React from "react";
import { SettingsWindow } from "../../kit/overlays/settings_window.jsx";

/** The kit Scrim fills this positioned frame; CSS scales the fixed-size window. */
export function SettingsAnatomy() {
  return (
    <div className="anat-settings">
      <div className="anat-settings__stage">
        <SettingsWindow theme="mocha" onTheme={() => {}} uiScale="md" onUiScale={() => {}} onClose={() => {}} />
      </div>
    </div>
  );
}
