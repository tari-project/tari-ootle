//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it, vi } from "vitest";
import { applyRenameSuccessIfActive, getValidatedRename } from "./accountNameHelpers";

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

  describe("applyRenameSuccessIfActive", () => {
    it("closes edit mode and notifies caller when the renamed account is still active", () => {
      const closeEdit = vi.fn();
      const onRenameSuccess = vi.fn();

      const applied = applyRenameSuccessIfActive(ACCOUNT_A, ACCOUNT_A, "Savings", {
        closeEdit,
        onRenameSuccess,
      });

      expect(applied).toBe(true);
      expect(closeEdit).toHaveBeenCalledTimes(1);
      expect(onRenameSuccess).toHaveBeenCalledWith("Savings");
    });

    it("skips callbacks when the user switched to another account before rename resolved", () => {
      const closeEdit = vi.fn();
      const onRenameSuccess = vi.fn();

      const applied = applyRenameSuccessIfActive(ACCOUNT_A, ACCOUNT_B, "Savings", {
        closeEdit,
        onRenameSuccess,
      });

      expect(applied).toBe(false);
      expect(closeEdit).not.toHaveBeenCalled();
      expect(onRenameSuccess).not.toHaveBeenCalled();
    });
  });
});
