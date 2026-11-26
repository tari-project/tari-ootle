//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { parseTimestamp } from "@utils/helpers";

const SECOND_MS = 1_000;
const MINUTE_MS = 60 * SECOND_MS;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const WEEK_MS = 7 * DAY_MS;

export function formatTimeAgo(rawTimestamp: string | null | undefined, nowMs: number = Date.now()): string {
  const date = parseTimestamp(rawTimestamp);
  if (!date) return "";

  const diffMs = Math.max(0, nowMs - date.getTime());
  const diffSeconds = Math.floor(diffMs / SECOND_MS);
  const diffMinutes = Math.floor(diffSeconds / 60);
  const diffHours = Math.floor(diffMinutes / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffSeconds < 60) return `${diffSeconds} sec${diffSeconds !== 1 ? "s" : ""} ago`;
  if (diffMinutes < 60) return `${diffMinutes} min${diffMinutes !== 1 ? "s" : ""} ago`;
  if (diffHours < 24) return `${diffHours} hour${diffHours !== 1 ? "s" : ""} ago`;
  if (diffDays === 1) return "yesterday";
  if (diffDays < 7) return `${diffDays} day${diffDays !== 1 ? "s" : ""} ago`;

  return date.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export function getNextTimeAgoRefreshMs(
  rawTimestamp: string | null | undefined,
  nowMs: number = Date.now(),
): number | null {
  const date = parseTimestamp(rawTimestamp);
  if (!date) return null;

  const rawDiffMs = nowMs - date.getTime();
  if (rawDiffMs < 0) {
    // Clock skew / future timestamp: re-check once the timestamp catches up (bounded to 1 minute).
    return Math.min(MINUTE_MS, Math.max(SECOND_MS, -rawDiffMs));
  }
  if (rawDiffMs < MINUTE_MS) {
    return SECOND_MS - (rawDiffMs % SECOND_MS);
  }
  if (rawDiffMs < HOUR_MS) {
    return MINUTE_MS - (rawDiffMs % MINUTE_MS);
  }
  if (rawDiffMs < WEEK_MS) {
    return HOUR_MS - (rawDiffMs % HOUR_MS);
  }
  // Dates >= 7 days old render as a static calendar date and no longer need polling.
  return null;
}

export function scheduleTimeAgoUpdates(
  rawTimestamp: string | null | undefined,
  onUpdate: (display: string) => void,
): () => void {
  let cancelled = false;
  let timerId: ReturnType<typeof setTimeout> | null = null;

  const tick = () => {
    if (cancelled) return;
    const currentNow = Date.now();
    onUpdate(formatTimeAgo(rawTimestamp, currentNow));

    const nextDelay = getNextTimeAgoRefreshMs(rawTimestamp, currentNow);
    if (nextDelay !== null && !cancelled) {
      timerId = setTimeout(tick, nextDelay);
    }
  };

  tick();

  return () => {
    cancelled = true;
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }
  };
}
