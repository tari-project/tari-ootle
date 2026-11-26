//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { getSubstateType, getSubstateTypeColor } from "./inputsHelpers";

describe("inputsHelpers", () => {
  it("maps every engine address prefix to its display type", () => {
    expect(getSubstateType("component_deadbeef")).toBe("Component");
    expect(getSubstateType("vault_deadbeef")).toBe("Vault");
    expect(getSubstateType("resource_deadbeef")).toBe("Resource");
    expect(getSubstateType("nft_deadbeef_u32_1")).toBe("NFT");
    expect(getSubstateType("tombstone_deadbeef")).toBe("Claimed Output Tombstone");
    expect(getSubstateType("txreceipt_deadbeef")).toBe("Transaction Receipt");
    expect(getSubstateType("template_deadbeef")).toBe("Template");
    expect(getSubstateType("utxo_deadbeef")).toBe("Utxo");
    expect(getSubstateType("vnfp_deadbeef")).toBe("VnFeePool");
    expect(getSubstateType("coutput_deadbeef")).toBe("Confidential Output");
    expect(getSubstateType("unrecognized_1234")).toBe("Unknown");
  });

  it("maps substate display types to chip colors", () => {
    expect(getSubstateTypeColor("Claimed Output Tombstone")).toBe("warning");
    expect(getSubstateTypeColor("Transaction Receipt")).toBe("error");
    expect(getSubstateTypeColor("Unknown")).toBe("primary");
  });
});
