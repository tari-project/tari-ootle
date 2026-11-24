// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import type { Account, AccountInfo } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { buildAccountPickerOptions, resolveFallbackAccount } from "./accountPickerHelpers";

function makeAccount(id: string, name: string | null, isDefault = false): Account {
  return {
    name,
    component_address: id,
    view_only_key_id: { Derived: { key_branch: "view_only_key", index: 0 } },
    owner_key_id: null,
    owner_public_key: "00".repeat(32),
    birthday_epoch: 0,
    is_confirmed_on_chain: true,
    is_default: isDefault,
  };
}

function makeAccountInfo(id: string, name: string | null, isDefault = false): AccountInfo {
  return {
    account: makeAccount(id, name, isDefault),
    address: `otl_loc_${id}`,
  };
}

describe("accountPickerHelpers", () => {
  it("prefers the default account and falls back to the first account when none is default", () => {
    const first = makeAccountInfo("component_01", "First", false);
    const second = makeAccountInfo("component_02", "Second", true);

    expect(resolveFallbackAccount(undefined)).toBeUndefined();
    expect(resolveFallbackAccount([])).toBeUndefined();
    expect(resolveFallbackAccount([first, second])).toBe(second);
    expect(resolveFallbackAccount([first])).toBe(first);
  });

  it("includes currentAccount as a fallback option while loading or outside the fetched page", () => {
    const active = makeAccount("component_99", "Overflow Account", false);
    const fetched = [makeAccountInfo("component_01", "Main", true)];

    expect(buildAccountPickerOptions(undefined, active)).toEqual([
      { address: "component_99", label: "Overflow Account" },
    ]);

    expect(buildAccountPickerOptions(fetched, active)).toEqual([
      { address: "component_99", label: "Overflow Account" },
      { address: "component_01", label: "Main" },
    ]);
  });

  it("does not duplicate currentAccount when it is already present in fetched accounts", () => {
    const main = makeAccountInfo("component_01", null, true);
    expect(buildAccountPickerOptions([main], main.account)).toEqual([
      { address: "component_01", label: "component_01" },
    ]);
  });
});
