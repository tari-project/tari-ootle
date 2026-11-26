//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { AccountsCreateFreeTestCoinsResponse } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it, vi } from "vitest";
import { addClaimedAccount, applyClaimSuccess, markAccountClaimed } from "./claimCoinsHelpers";

const ACCOUNT_A = "component_0000000000000000000000000000000000000000000000000000000000000001";
const ACCOUNT_B = "component_0000000000000000000000000000000000000000000000000000000000000002";

const SAMPLE_RESP = {
  account: { component_address: ACCOUNT_A },
  address: "xtr_test_addr_a",
} as unknown as AccountsCreateFreeTestCoinsResponse;

describe("claimCoinsHelpers", () => {
  describe("addClaimedAccount", () => {
    it("appends a newly claimed account without mutating the input array", () => {
      const existing = [ACCOUNT_A];
      const updated = addClaimedAccount(existing, ACCOUNT_B);
      expect(updated).toEqual([ACCOUNT_A, ACCOUNT_B]);
      expect(existing).toEqual([ACCOUNT_A]);
    });

    it("returns null when the account is already recorded as claimed", () => {
      expect(addClaimedAccount([ACCOUNT_A, ACCOUNT_B], ACCOUNT_A)).toBeNull();
    });
  });

  describe("applyClaimSuccess", () => {
    it("updates active account state when the user is still viewing the claimed account", () => {
      const setAccount = vi.fn();
      const setOotleAddress = vi.fn();

      applyClaimSuccess(ACCOUNT_A, SAMPLE_RESP, {
        getActiveAccount: () => ACCOUNT_A,
        setAccount,
        setOotleAddress,
      });

      expect(setAccount).toHaveBeenCalledWith(SAMPLE_RESP.account);
      expect(setOotleAddress).toHaveBeenCalledWith(SAMPLE_RESP.address);
    });

    it("skips overwriting active account state if the user switched accounts mid-claim", () => {
      const setAccount = vi.fn();
      const setOotleAddress = vi.fn();

      applyClaimSuccess(ACCOUNT_A, SAMPLE_RESP, {
        getActiveAccount: () => ACCOUNT_B,
        setAccount,
        setOotleAddress,
      });

      expect(setAccount).not.toHaveBeenCalled();
      expect(setOotleAddress).not.toHaveBeenCalled();
    });
  });

  describe("markAccountClaimed", () => {
    it("marks UI claimed and persists when the claimed account is still active", async () => {
      const setHasClaimed = vi.fn();
      const saveClaimedAccounts = vi.fn().mockResolvedValue(undefined);

      await markAccountClaimed(ACCOUNT_A, {
        getActiveAccount: () => ACCOUNT_A,
        setHasClaimed,
        getSettings: async () => ({ claimed_accounts: [] }),
        saveClaimedAccounts,
      });

      expect(setHasClaimed).toHaveBeenCalledWith(true);
      expect(saveClaimedAccounts).toHaveBeenCalledWith([ACCOUNT_A]);
    });

    it("persists the claimed account without marking the newly selected account as claimed", async () => {
      const setHasClaimed = vi.fn();
      const saveClaimedAccounts = vi.fn().mockResolvedValue(undefined);

      await markAccountClaimed(ACCOUNT_A, {
        getActiveAccount: () => ACCOUNT_B,
        setHasClaimed,
        getSettings: async () => ({ claimed_accounts: [] }),
        saveClaimedAccounts,
      });

      expect(setHasClaimed).not.toHaveBeenCalled();
      expect(saveClaimedAccounts).toHaveBeenCalledWith([ACCOUNT_A]);
    });
  });
});
