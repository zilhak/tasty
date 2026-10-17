import React from "react";
import { Icon } from "../core/Icon";

/**
 * Tasty NotificationBadge — unread count for ONE notification level (2026-10-17).
 * A 16 pill: level glyph (10) + mono count. Glyph shape carries the level without
 * colour (error = alertCircle, warning = alertTriangle, info = info); the pill
 * also carries an aria-label / title ("3 errors").
 *   count <= 0 → nothing is drawn (no "0" pill)
 *   count > max (99) → "99+"
 * NotificationBadgeGroup draws the three levels in the fixed order
 * error › warning › info, skipping zeros; row (LNB, popup toolbar) or column (rail).
 * Separate axis from surface attention: never sum these with Badge counts.
 */
export const NOTIFICATION_LEVELS = {
  error:   { glyph: "alertCircle",   one: "error",   many: "errors" },
  warning: { glyph: "alertTriangle", one: "warning", many: "warnings" },
  info:    { glyph: "info",          one: "info",    many: "info" },
};

export function notificationCountLabel(level, count) {
  const L = NOTIFICATION_LEVELS[level] || NOTIFICATION_LEVELS.info;
  return count + " " + (count === 1 ? L.one : L.many);
}

export function NotificationBadge({ level = "info", count = 0, max = 99, glyph = true, style, ...rest }) {
  if (!count || count <= 0) return null;
  const L = NOTIFICATION_LEVELS[level] || NOTIFICATION_LEVELS.info;
  const text = count > max ? max + "+" : String(count);
  const label = notificationCountLabel(level, count);
  return (
    <span role="img" aria-label={label} title={label} {...rest}
      style={{ display: "inline-flex", alignItems: "center", justifyContent: "center", flex: "none", boxSizing: "border-box",
        gap: "var(--tasty-notification-badge-glyph-gap)", height: "var(--tasty-badge-size)", minWidth: "var(--tasty-badge-size)",
        padding: "0 var(--tasty-badge-padding-x)", borderRadius: "var(--tasty-radius-pill)",
        fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-badge-font-size)", fontWeight: "var(--tasty-font-weight-bold)",
        lineHeight: "var(--tasty-line-height-tight)", whiteSpace: "nowrap",
        background: "var(--tasty-notification-badge-" + level + "-bg)", color: "var(--tasty-notification-badge-" + level + "-fg)", ...style }}>
      {glyph && <Icon name={L.glyph} size="var(--tasty-notification-badge-glyph-size)" aria-hidden="true" />}
      <span>{text}</span>
    </span>
  );
}

export function NotificationBadgeGroup({ error = 0, warning = 0, info = 0, direction = "row", max = 99, glyph = true, style, ...rest }) {
  if (error <= 0 && warning <= 0 && info <= 0) return null;
  const col = direction === "column";
  return (
    <span {...rest} style={{ display: "inline-flex", flexDirection: col ? "column" : "row", alignItems: "center", flex: "none",
      gap: col ? "var(--tasty-notification-badge-stack-gap)" : "var(--tasty-notification-badge-group-gap)", ...style }}>
      <NotificationBadge level="error" count={error} max={max} glyph={glyph} />
      <NotificationBadge level="warning" count={warning} max={max} glyph={glyph} />
      <NotificationBadge level="info" count={info} max={max} glyph={glyph} />
    </span>
  );
}
