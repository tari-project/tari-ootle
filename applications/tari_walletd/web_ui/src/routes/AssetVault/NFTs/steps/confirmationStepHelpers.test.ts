//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { NonFungibleToken } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { findSelectedNft, parseMaxFeeAmount } from "./confirmationStepHelpers";

function makeNft(nft_id: NonFungibleToken["nft_id"]): NonFungibleToken {
  return {
    resource_address: "resource_0000000000000000000000000000000000000000000000000000000000000000",
    vault_id: "vault_0000000000000000000000000000000000000000000000000000000000000000",
    nft_id,
    data: { Null: null },
    mutable_data: { Null: null },
    is_burnt: false,
  };
}

describe("confirmationStepHelpers", () => {
  describe("parseMaxFeeAmount", () => {
    it("parses valid unsigned integer fee strings including values above MAX_SAFE_INTEGER", () => {
      expect(parseMaxFeeAmount("0")).toBe(0n);
      expect(parseMaxFeeAmount(" 2500 ")).toBe(2500n);
      expect(parseMaxFeeAmount("9007199254740993")).toBe(9007199254740993n);
    });

    it("returns null for empty, non-numeric, negative, decimal, or partially numeric strings", () => {
      expect(parseMaxFeeAmount("")).toBeNull();
      expect(parseMaxFeeAmount("   ")).toBeNull();
      expect(parseMaxFeeAmount("abc")).toBeNull();
      expect(parseMaxFeeAmount("123abc")).toBeNull();
      expect(parseMaxFeeAmount("-100")).toBeNull();
      expect(parseMaxFeeAmount("12.5")).toBeNull();
    });
  });

  describe("findSelectedNft", () => {
    it("matches NFTs across U256, Uint64, Uint32, and String ID variants", () => {
      const u256Nft = makeNft({ U256: "aabbccdd" });
      const u64Nft = makeNft({ Uint64: 99 });
      const strNft = makeNft({ String: "rare-1" });
      const list = [u256Nft, u64Nft, strNft];

      expect(findSelectedNft(list, { U256: "aabbccdd" })).toBe(u256Nft);
      expect(findSelectedNft(list, { Uint64: 99 })).toBe(u64Nft);
      expect(findSelectedNft(list, { String: "rare-1" })).toBe(strNft);
      expect(findSelectedNft(list, { Uint32: 7 })).toBeUndefined();
    });

    it("returns undefined when preSelectedNftId or availableNfts is missing", () => {
      expect(findSelectedNft(undefined, { Uint32: 1 })).toBeUndefined();
      expect(findSelectedNft([makeNft({ Uint32: 1 })], undefined)).toBeUndefined();
    });
  });
});
