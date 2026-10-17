import React from "react";
import { Spinner } from "./Spinner";

/**
 * Tasty LoadingOverlay — a Spinner laid over content that is already on screen
 * (a surface, a tab's kind-icon slot, a workspace card's dot slot, a rail avatar)
 * while a slow request runs. It adds no slot and changes no geometry; it never
 * takes pointer or keyboard input; it owns no lifetime (the caller passes
 * `visible` and, after loading-overlay-delay, mounts it).
 *
 * Placement by `kind` — the caller positions the parent (or passes `rect`):
 *   surface  rect = surface rect · 24 bed (surface-raised, 1px border-default) bottom-right, inset 8 · spinner 16
 *   tab      rect = the tab's 14 kind-icon slot · bed = the cell's current fill · spinner 14
 *   lnb      rect = the 16 workspace-dot-slot (title-line height) · bed = the row's current fill · spinner 14
 *   rail     rect = the 36 avatar box · no bed · 28 ring around the initial
 * Ink = loading-overlay-indicator (→ spinner-indicator → accent-primary) everywhere.
 * Reduced motion = the still arc (not the 3-dot fallback).
 */
const SIZE = {
  surface: "var(--tasty-loading-overlay-surface-size)",
  tab: "var(--tasty-loading-overlay-tab-size)",
  lnb: "var(--tasty-loading-overlay-lnb-size)",
  rail: "var(--tasty-loading-overlay-rail-size)",
};

export function LoadingOverlay({ visible = true, kind = "surface", rect, bed, label = "Loading", reducedMotion = false, style, ...rest }) {
  if (!visible) return null;
  const box = rect
    ? { left: rect.x, top: rect.y, width: rect.width, height: rect.height }
    : { inset: 0 };
  const spinner = (
    <Spinner size={SIZE[kind] || SIZE.surface} stroke={kind === "rail" ? 1.72 : 2} label={label}
      paused={reducedMotion} reducedMotion="arc"
      style={{ color: "var(--tasty-loading-overlay-indicator)" }} />
  );
  const outer = {
    position: "absolute", ...box, pointerEvents: "none", boxSizing: "border-box",
    display: "flex", alignItems: "center", justifyContent: "center",
  };
  if (kind === "surface") {
    return (
      <span data-loading-overlay="surface" style={{ ...outer, alignItems: "flex-end", justifyContent: "flex-end", padding: "var(--tasty-loading-overlay-surface-inset)", ...style }} {...rest}>
        <span style={{ width: "var(--tasty-loading-overlay-surface-bed-size)", height: "var(--tasty-loading-overlay-surface-bed-size)", boxSizing: "border-box",
          display: "inline-flex", alignItems: "center", justifyContent: "center", borderRadius: "var(--tasty-radius)",
          background: "var(--tasty-loading-overlay-surface-bed-bg)", border: "var(--tasty-border-width) solid var(--tasty-loading-overlay-surface-bed-border)" }}>{spinner}</span>
      </span>
    );
  }
  return (
    <span data-loading-overlay={kind} style={{ ...outer, background: kind === "rail" ? "transparent" : (bed || "var(--tasty-bg-sidebar)"), ...style }} {...rest}>{spinner}</span>
  );
}
