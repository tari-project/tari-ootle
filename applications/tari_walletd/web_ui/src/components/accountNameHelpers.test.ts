//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it, vi } from "vitest";
import { getValidatedRename, runIfAccountActive } from "./accountNameHelpers";

const ACCOUNT_A = "component_0000000000000000000000000000000000000000000000000000000000000001";
const ACCOUNT_B = "component_0000000000000000000000000000000000000000000000000000000000000002";

describe("accountNameHelpers", () => {
  describe("getValidatedRename", () => {
    it("returns the trimmed name when changed", () => {
      expect(getValidatedRename("  Treasury  ", "Main")).toBe("Treasury");
      expect(getValidatedRename("Main", null)).toBe("Main");
      expect(getValidatedRename("Main", undefined)).toBe("Main");
    });

    it("returns null for empty, whitespace-only, or unchanged trimmed names", () => {
      expect(getValidatedRename("", "Main")).toBeNull();
      expect(getValidatedRename("   ", "Main")).toBeNull();
      expect(getValidatedRename("Main", "Main")).toBeNull();
      expect(getValidatedRename("  Main  ", "Main")).toBeNull();
    });
  });

  describe("runIfAccountActive", () => {
    it("runs callback when the renamed account is still active", () => {
      const cb = vi.fn();
      expect(runIfAccountActive(ACCOUNT_A, ACCOUNT_A, cb)).toBe(true);
      expect(cb).toHaveBeenCalledTimes(1);
    });

    it("skips callback when the user switched to another account before rename settled", () => {
      const cb = vi.fn();
      expect(runIfAccountActive(ACCOUNT_A, ACCOUNT_B, cb)).toBe(false);
      expect(cb).not.toHaveBeenCalled();
    });
  });
});
