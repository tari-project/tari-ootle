//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { afterEach, describe, expect, it, vi } from "vitest";
import { formatTimeAgo, getNextTimeAgoRefreshMs, scheduleTimeAgoUpdates } from "./timeAgo";

const BASE_ISO = "2026-11-24T12:00:00.000Z";
const BASE_MS = Date.parse(BASE_ISO);

describe("timeAgo", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  describe("formatTimeAgo", () => {
    it("returns empty string for null, undefined, or invalid timestamps", () => {
      expect(formatTimeAgo(null, BASE_MS)).toBe("");
      expect(formatTimeAgo(undefined, BASE_MS)).toBe("");
      expect(formatTimeAgo("not-a-timestamp", BASE_MS)).toBe("");
    });

    it("clamps future timestamps (clock skew) instead of returning negative seconds", () => {
      expect(formatTimeAgo(BASE_ISO, BASE_MS - 5_000)).toBe("0 secs ago");
      expect(formatTimeAgo(BASE_ISO, BASE_MS - 1_000)).toBe("0 secs ago");
    });

    it("formats relative durations accurately across units", () => {
      expect(formatTimeAgo(BASE_ISO, BASE_MS + 1_000)).toBe("1 sec ago");
      expect(formatTimeAgo(BASE_ISO, BASE_MS + 42_000)).toBe("42 secs ago");
      expect(formatTimeAgo(BASE_ISO, BASE_MS + 60_000)).toBe("1 min ago");
      expect(formatTimeAgo(BASE_ISO, BASE_MS + 5 * 60_000)).toBe("5 mins ago");
      expect(formatTimeAgo(BASE_ISO, BASE_MS + 3_600_000)).toBe("1 hour ago");
      expect(formatTimeAgo(BASE_ISO, BASE_MS + 2 * 3_600_000)).toBe("2 hours ago");
      expect(formatTimeAgo(BASE_ISO, BASE_MS + 24 * 3_600_000)).toBe("yesterday");
      expect(formatTimeAgo(BASE_ISO, BASE_MS + 3 * 24 * 3_600_000)).toBe("3 days ago");
    });

    it("formats timestamps 7+ days old as a calendar date", () => {
      const formatted = formatTimeAgo(BASE_ISO, BASE_MS + 10 * 24 * 3_600_000);
      expect(formatted).not.toContain("ago");
      expect(formatted).toContain("2026");
    });
  });

  describe("getNextTimeAgoRefreshMs", () => {
    it("returns null for invalid timestamps or timestamps >= 7 days old", () => {
      expect(getNextTimeAgoRefreshMs(null, BASE_MS)).toBeNull();
      expect(getNextTimeAgoRefreshMs("invalid", BASE_MS)).toBeNull();
      expect(getNextTimeAgoRefreshMs(BASE_ISO, BASE_MS + 7 * 24 * 3_600_000)).toBeNull();
    });

    it("aligns delays to the next second, minute, or hour boundary", () => {
      expect(getNextTimeAgoRefreshMs(BASE_ISO, BASE_MS + 250)).toBe(750);
      expect(getNextTimeAgoRefreshMs(BASE_ISO, BASE_MS + 65_000)).toBe(55_000);
      expect(getNextTimeAgoRefreshMs(BASE_ISO, BASE_MS + 3_600_000 + 10 * 60_000)).toBe(50 * 60_000);
    });

    it("bounds future timestamp refresh delays to [1s, 60s]", () => {
      expect(getNextTimeAgoRefreshMs(BASE_ISO, BASE_MS - 300)).toBe(1_000);
      expect(getNextTimeAgoRefreshMs(BASE_ISO, BASE_MS - 4_500)).toBe(4_500);
      expect(getNextTimeAgoRefreshMs(BASE_ISO, BASE_MS - 600_000)).toBe(60_000);
    });
  });

  describe("scheduleTimeAgoUpdates", () => {
    it("updates every second under 1 minute, transitions to minutes, and stops on cleanup", () => {
      vi.useFakeTimers();
      vi.setSystemTime(BASE_MS + 58_000);

      const updates: string[] = [];
      const stop = scheduleTimeAgoUpdates(BASE_ISO, (val) => updates.push(val));

      expect(updates).toEqual(["58 secs ago"]);

      vi.advanceTimersByTime(1_000);
      expect(updates).toEqual(["58 secs ago", "59 secs ago"]);

      vi.advanceTimersByTime(1_000);
      expect(updates).toEqual(["58 secs ago", "59 secs ago", "1 min ago"]);

      // Now on 1-minute cadence: advancing 30s should not fire yet
      vi.advanceTimersByTime(30_000);
      expect(updates).toHaveLength(3);

      vi.advanceTimersByTime(30_000);
      expect(updates).toEqual(["58 secs ago", "59 secs ago", "1 min ago", "2 mins ago"]);

      stop();
      vi.advanceTimersByTime(120_000);
      expect(updates).toHaveLength(4);
    });

    it("does not schedule any timer for invalid or >= 7-day-old timestamps", () => {
      vi.useFakeTimers();
      vi.setSystemTime(BASE_MS + 10 * 24 * 3_600_000);

      const updates: string[] = [];
      const stopOld = scheduleTimeAgoUpdates(BASE_ISO, (val) => updates.push(val));
      expect(vi.getTimerCount()).toBe(0);
      stopOld();

      const stopInvalid = scheduleTimeAgoUpdates(null, (val) => updates.push(val));
      expect(vi.getTimerCount()).toBe(0);
      stopInvalid();
    });
  });
});
