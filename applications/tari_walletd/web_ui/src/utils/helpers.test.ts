//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { formatTimestamp, parseTimestamp } from "./helpers";

describe("parseTimestamp", () => {
  it("parses ISO-8601 UTC timestamps ending with Z", () => {
    expect(parseTimestamp("2026-11-23T15:00:00Z")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
    expect(parseTimestamp("2026-11-23T15:00:00.123Z")?.toISOString()).toBe("2026-11-23T15:00:00.123Z");
    expect(parseTimestamp("2026-11-23T15:00:00z")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
  });

  it("parses naive timestamps as UTC", () => {
    expect(parseTimestamp("2026-11-23 15:00:00")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
    expect(parseTimestamp("2026-11-23 15:00:00.0")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
    expect(parseTimestamp("2026-11-23T15:00:00")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
  });

  it("preserves explicit numeric timezone offsets", () => {
    expect(parseTimestamp("2026-11-23T17:00:00+02:00")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
    expect(parseTimestamp("2026-11-23T10:00:00-0500")?.toISOString()).toBe("2026-11-23T15:00:00.000Z");
  });

  it("returns null for empty, nullish, or invalid timestamps", () => {
    expect(parseTimestamp(null)).toBeNull();
    expect(parseTimestamp(undefined)).toBeNull();
    expect(parseTimestamp("")).toBeNull();
    expect(parseTimestamp("   ")).toBeNull();
    expect(parseTimestamp("not-a-timestamp")).toBeNull();
  });
});

describe("formatTimestamp", () => {
  it("formats UTC Z-suffixed timestamps instead of returning an empty string", () => {
    const formatted = formatTimestamp("2026-11-23T15:00:00Z");
    expect(formatted).not.toBe("");
    expect(formatted).toContain("2026");
  });

  it("returns an empty string for nullish or invalid timestamps", () => {
    expect(formatTimestamp(null)).toBe("");
    expect(formatTimestamp(undefined)).toBe("");
    expect(formatTimestamp("invalid")).toBe("");
  });
});
