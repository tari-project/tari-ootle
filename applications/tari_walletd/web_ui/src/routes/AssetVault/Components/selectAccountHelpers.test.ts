//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { Account, AccountInfo, ComponentAddress } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { buildAccountSelectOptions, resolveSelectedAccountValue } from "./selectAccountHelpers";

const ADDR_1 = "component_0000000000000000000000000000000000000000000000000000000000000001" as ComponentAddress;
const ADDR_2 = "component_0000000000000000000000000000000000000000000000000000000000000002" as ComponentAddress;

function makeAccount(address: ComponentAddress, name: string | null): Account {
  return { component_address: address, name } as unknown as Account;
}

function makeAccountInfo(address: ComponentAddress, name: string | null): AccountInfo {
  return { account: makeAccount(address, name), address: `xtr_${address}` } as unknown as AccountInfo;
}

describe("selectAccountHelpers", () => {
  it("includes fetched accounts without duplicating the active account", () => {
    const fetched = [makeAccountInfo(ADDR_1, "Main"), makeAccountInfo(ADDR_2, null)];
    const options = buildAccountSelectOptions(fetched, makeAccount(ADDR_1, "Main"));

    expect(options).toEqual([
      { address: ADDR_1, label: "Main" },
      { address: ADDR_2, label: "<No Name>" },
    ]);
    expect(resolveSelectedAccountValue(options, makeAccount(ADDR_1, "Main"))).toBe(ADDR_1);
  });

  it("retains an active account missing from the fetched page instead of falling back to the first account", () => {
    const fetched = [makeAccountInfo(ADDR_1, "Main")];
    const active = makeAccount(ADDR_2, "Account 11");
    const options = buildAccountSelectOptions(fetched, active);

    expect(options).toEqual([
      { address: ADDR_1, label: "Main" },
      { address: ADDR_2, label: "Account 11" },
    ]);
    expect(resolveSelectedAccountValue(options, active)).toBe(ADDR_2);
  });

  it("returns an empty string instead of addAccount while accounts are loading or empty", () => {
    const options = buildAccountSelectOptions(undefined, null);
    expect(options).toEqual([]);
    expect(resolveSelectedAccountValue(options, null)).toBe("");
  });
});
