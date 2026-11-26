// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import {
  buildAddressOptions,
  filterAddressOptions,
  formatAddressOptionLabel,
  resolveSelectedAddressOption,
} from "./addressAutocompleteHelpers";

const ALICE_ADDR = "otl_loc_1alice000000middlefragment99999900000000000000000000000000000000xyz";
const BOB_ADDR = "otl_loc_1bob00000000otherpart7777770000000000000000000000000000000000000abc";

describe("addressAutocompleteHelpers", () => {
  it("formats unnamed accounts without a leading space", () => {
    expect(formatAddressOptionLabel(null, ALICE_ADDR)).toBe("otl_loc_...00000xyz");
    expect(formatAddressOptionLabel("   ", ALICE_ADDR)).toBe("otl_loc_...00000xyz");
    expect(formatAddressOptionLabel("Alice", ALICE_ADDR)).toBe("Alice (otl_loc_...00000xyz)");
  });

  it("assigns distinct keys even when an account and a contact share the same address", () => {
    const options = buildAddressOptions(
      [{ account: { name: "Treasury" }, address: ALICE_ADDR }],
      [{ name: "Treasury", address: ALICE_ADDR }],
    );

    expect(options).toHaveLength(2);
    expect(options[0].key).not.toBe(options[1].key);
    expect(options[0].value).toBe(ALICE_ADDR);
    expect(options[1].value).toBe(ALICE_ADDR);
  });

  it("matches options by name or by full address substring hidden by shortening", () => {
    const options = buildAddressOptions(
      [{ account: { name: "Main" }, address: ALICE_ADDR }],
      [{ name: "Bob", address: BOB_ADDR }],
    );

    expect(filterAddressOptions(options, "")).toHaveLength(2);
    expect(filterAddressOptions(options, "middlefragment999")).toEqual([options[0]]);
    expect(filterAddressOptions(options, "OTHERPART777")).toEqual([options[1]]);
    expect(filterAddressOptions(options, "  bob  ")).toEqual([options[1]]);
    expect(filterAddressOptions(options, "no-such-match")).toEqual([]);
  });

  it("keeps controlled selection synced with value so edited text clears option match", () => {
    const options = buildAddressOptions([{ account: { name: "Alice" }, address: ALICE_ADDR }], []);

    expect(resolveSelectedAddressOption(options, "")).toBeNull();
    expect(resolveSelectedAddressOption(options, ALICE_ADDR)).toBe(options[0]);
    expect(resolveSelectedAddressOption(options, ALICE_ADDR.slice(0, -1))).toBe(ALICE_ADDR.slice(0, -1));
  });
});
