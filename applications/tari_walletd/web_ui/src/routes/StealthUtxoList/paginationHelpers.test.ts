//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { clampPageIndex } from "./paginationHelpers";

describe("clampPageIndex", () => {
  it("returns 0 when the list is empty or rowsPerPage is non-positive", () => {
    expect(clampPageIndex(2, 10, 0)).toBe(0);
    expect(clampPageIndex(0, 10, 0)).toBe(0);
    expect(clampPageIndex(2, 0, 25)).toBe(0);
    expect(clampPageIndex(-1, 10, 25)).toBe(0);
  });

  it("keeps the current page when within bounds", () => {
    expect(clampPageIndex(0, 10, 25)).toBe(0);
    expect(clampPageIndex(1, 10, 25)).toBe(1);
    expect(clampPageIndex(2, 10, 25)).toBe(2);
  });

  it("clamps to the last valid page when totalItems shrinks", () => {
    expect(clampPageIndex(3, 10, 25)).toBe(2);
    expect(clampPageIndex(2, 10, 5)).toBe(0);
  });

  it("handles exact multiples of rowsPerPage without overshooting", () => {
    expect(clampPageIndex(1, 10, 10)).toBe(0);
    expect(clampPageIndex(2, 10, 20)).toBe(1);
    expect(clampPageIndex(1, 10, 20)).toBe(1);
  });
});
