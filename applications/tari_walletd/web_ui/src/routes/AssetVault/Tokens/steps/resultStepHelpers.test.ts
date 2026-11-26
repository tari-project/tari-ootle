//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { shouldAutoCloseTransferResult } from "./resultStepHelpers";

describe("shouldAutoCloseTransferResult", () => {
  it("returns true only when finished and transfer succeeded", () => {
    expect(shouldAutoCloseTransferResult(false, { success: true })).toBe(true);
  });

  it("returns false while submitting, when result is missing, or when transfer failed", () => {
    expect(shouldAutoCloseTransferResult(true, { success: true })).toBe(false);
    expect(shouldAutoCloseTransferResult(false, undefined)).toBe(false);
    expect(shouldAutoCloseTransferResult(false, { success: false })).toBe(false);
  });
});
