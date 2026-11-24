//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { formatTimeChipTooltip } from "./timeChipHelpers";

describe("formatTimeChipTooltip", () => {
  it("prefixes formatted timestamps with Last updated by default and supports custom prefixes", () => {
    expect(formatTimeChipTooltip("2026-11-24 09:00:00")).toBe("Last updated: 2026-11-24 09:00:00");
    expect(formatTimeChipTooltip("2026-11-24 09:00:00", "Created at")).toBe("Created at: 2026-11-24 09:00:00");
    expect(formatTimeChipTooltip("2026-11-24 09:00:00", "   ")).toBe("2026-11-24 09:00:00");
  });

  it("returns an empty string when formattedTime is blank", () => {
    expect(formatTimeChipTooltip("")).toBe("");
    expect(formatTimeChipTooltip("   ")).toBe("");
  });
});
