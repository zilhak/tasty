import * as React from "react";
import { NotificationLevel } from "./NotificationBadge";

export interface NotificationItemProps extends React.HTMLAttributes<HTMLDivElement> {
  level?: NotificationLevel;
  title?: string;
  /** Full body. Clamped to 2 lines (3 when wide); "More" appears only when cut. */
  body?: string;
  /** "workspace › surface". Kept after the source closes so the record stays readable. */
  source?: string;
  /** Source workspace/surface is gone → "· closed" tag, Jump disabled ("Source closed"). */
  sourceClosed?: boolean;
  /** Elapsed time, e.g. "2m", "1h", "3d". */
  time?: string;
  unread?: boolean;
  /** Body shown in full and wrapping. Caller marks the item read when it expands. */
  expanded?: boolean;
  /** Fullscreen stage — body clamp 3 lines. */
  wide?: boolean;
  /** Keyboard selection / forced states for specimens. */
  selected?: boolean;
  hover?: boolean;
  onToggle?: (e: React.MouseEvent) => void;
  onMarkRead?: (e: React.MouseEvent) => void;
  onJump?: (e: React.MouseEvent) => void;
}

/** One notifications-list row: level glyph · title · time · unread dot / body / source · mark read · Jump. */
export function NotificationItem(props: NotificationItemProps): JSX.Element;
