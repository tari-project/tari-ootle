//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { AccountInfo } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import {
  canSubmitClaimFees,
  findAccountAddressForClaimKey,
  parseOptionalFee,
  sumScannedFees,
} from "./claimFeesHelpers";

function makeAccountInfo(componentAddress: string, ownerKeyId: AccountInfo["account"]["owner_key_id"]): AccountInfo {
  return {
    account: {
      name: "test",
      component_address: componentAddress,
      view_only_key_id: { Derived: { key_branch: "view_only_key", index: 0 } },
      owner_key_id: ownerKeyId,
      owner_public_key: "00".repeat(32),
      birthday_epoch: 0,
      is_confirmed_on_chain: true,
      is_default: false,
    },
    address: "otl_loc_test",
  };
}

describe("findAccountAddressForClaimKey", () => {
  it("matches derived account keys regardless of index representation (number, bigint, string)", () => {
    const accounts = [
      makeAccountInfo("component_0000", { Derived: { key_branch: "account", index: 0 } }),
      makeAccountInfo("component_0001", { Derived: { key_branch: "account", index: 1n } }),
      makeAccountInfo("component_0002", { Derived: { key_branch: "account", index: "2" } }),
    ];

    expect(findAccountAddressForClaimKey(accounts, 0)).toBe("component_0000");
    expect(findAccountAddressForClaimKey(accounts, 1)).toBe("component_0001");
    expect(findAccountAddressForClaimKey(accounts, 2)).toBe("component_0002");
  });

  it("ignores imported keys, non-account branches, and null owner_key_id", () => {
    const accounts = [
      makeAccountInfo("component_null", null),
      makeAccountInfo("component_imported", { Imported: { local_key_id: 1 } }),
      makeAccountInfo("component_other_branch", { Derived: { key_branch: "view_only_key", index: 1 } }),
      makeAccountInfo("component_match", { Derived: { key_branch: "account", index: 1 } }),
    ];

    expect(findAccountAddressForClaimKey(accounts, 1)).toBe("component_match");
    expect(findAccountAddressForClaimKey(accounts, 99)).toBeNull();
    expect(findAccountAddressForClaimKey(undefined, 0)).toBeNull();
  });
});

describe("sumScannedFees", () => {
  it("sums fee pool amounts in base units across shards", () => {
    const total = sumScannedFees({
      "0": { address: "vnfp_00", amount: 500_000n },
      "1": { address: "vnfp_01", amount: 1_250_000n },
      "2": undefined,
    });
    expect(total).toBe(1_750_000n);
  });

  it("returns 0n for an empty map", () => {
    expect(sumScannedFees({})).toBe(0n);
  });
});

describe("parseOptionalFee", () => {
  it("treats empty or whitespace input as empty (estimate mode)", () => {
    expect(parseOptionalFee("")).toEqual({ kind: "empty" });
    expect(parseOptionalFee("   ")).toEqual({ kind: "empty" });
  });

  it("parses positive integer strings as bigint", () => {
    expect(parseOptionalFee("500")).toEqual({ kind: "valid", value: 500n });
    expect(parseOptionalFee(" 12345678901234567890 ")).toEqual({ kind: "valid", value: 12345678901234567890n });
  });

  it("rejects zero, negative, fractional, and non-numeric inputs", () => {
    expect(parseOptionalFee("0")).toEqual({ kind: "invalid" });
    expect(parseOptionalFee("-10")).toEqual({ kind: "invalid" });
    expect(parseOptionalFee("0.5")).toEqual({ kind: "invalid" });
    expect(parseOptionalFee("1e6")).toEqual({ kind: "invalid" });
    expect(parseOptionalFee("abc")).toEqual({ kind: "invalid" });
  });
});

describe("canSubmitClaimFees", () => {
  it("requires a selected key, at least one shard, and a non-invalid fee", () => {
    expect(canSubmitClaimFees(null, [1], "")).toBe(false);
    expect(canSubmitClaimFees(0, [], "")).toBe(false);
    expect(canSubmitClaimFees(0, [1], "0.5")).toBe(false);
    expect(canSubmitClaimFees(0, [1], "0")).toBe(false);
    expect(canSubmitClaimFees(0, [1], "")).toBe(true);
    expect(canSubmitClaimFees(0, [1], "500")).toBe(true);
  });
});
