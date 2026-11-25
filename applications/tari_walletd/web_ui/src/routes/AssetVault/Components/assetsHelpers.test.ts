//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { clampNftPage } from "./assetsHelpers";

describe("assetsHelpers", () => {
  describe("clampNftPage", () => {
    it("resets page to 0 when totalCount drops to 0", () => {
      expect(clampNftPage(2, 0, 12)).toBe(0);
      expect(clampNftPage(1, 0, 12)).toBe(0);
    });

    it("clamps page to the last valid page when items shrink", () => {
      expect(clampNftPage(3, 24, 12)).toBe(1);
      expect(clampNftPage(2, 12, 12)).toBe(0);
      expect(clampNftPage(2, 13, 12)).toBe(1);
    });

    it("preserves page when already within valid bounds", () => {
      expect(clampNftPage(0, 25, 12)).toBe(0);
      expect(clampNftPage(1, 25, 12)).toBe(1);
      expect(clampNftPage(2, 25, 12)).toBe(2);
    });

    it("returns 0 for negative pages or non-positive rowsPerPage", () => {
      expect(clampNftPage(-1, 25, 12)).toBe(0);
      expect(clampNftPage(2, 25, 0)).toBe(0);
    });
  });
});
