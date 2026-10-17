import * as React from "react";

export type NotificationLevel = "error" | "warning" | "info";

export interface NotificationBadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** error = red (critical failure) · warning = yellow (non-critical failure) · info = completion blue. */
  level?: NotificationLevel;
  /** Unread count for this level. 0 or less draws nothing. */
  count?: number;
  /** Above this the pill reads "99+". Default 99 (--tasty-notification-badge-max). */
  max?: number;
  /** Show the level glyph before the number. Default true. */
  glyph?: boolean;
}

/** Unread count pill for one notification level — glyph + number, aria-label "3 errors". */
export function NotificationBadge(props: NotificationBadgeProps): JSX.Element | null;

export interface NotificationBadgeGroupProps extends React.HTMLAttributes<HTMLSpanElement> {
  error?: number;
  warning?: number;
  info?: number;
  /** row = LNB / popup toolbar · column = collapsed rail. */
  direction?: "row" | "column";
  max?: number;
  glyph?: boolean;
}

/** The three level badges in fixed order error › warning › info, zeros skipped. Null when all are 0. */
export function NotificationBadgeGroup(props: NotificationBadgeGroupProps): JSX.Element | null;

/** "1 error" / "3 warnings" / "5 info" — the tooltip and aria string. */
export function notificationCountLabel(level: NotificationLevel, count: number): string;

export const NOTIFICATION_LEVELS: Record<NotificationLevel, { glyph: string; one: string; many: string }>;
