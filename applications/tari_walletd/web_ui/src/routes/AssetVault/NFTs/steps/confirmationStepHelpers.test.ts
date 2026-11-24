//  Copyright 2025. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import type { NonFungibleToken } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { findSelectedNft, getNftAvatarImageUrl, parseMaxFeeAmount } from "./confirmationStepHelpers";

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

  describe("getNftAvatarImageUrl", () => {
    it("extracts and trims valid image_url strings from plain objects", () => {
      expect(getNftAvatarImageUrl({ image_url: " https://example.com/nft.png " })).toBe("https://example.com/nft.png");
    });

    it("returns undefined for non-object, array, missing, blank, or non-string image_url values", () => {
      expect(getNftAvatarImageUrl(null)).toBeUndefined();
      expect(getNftAvatarImageUrl("string")).toBeUndefined();
      expect(getNftAvatarImageUrl(["https://example.com/nft.png"])).toBeUndefined();
      expect(getNftAvatarImageUrl({})).toBeUndefined();
      expect(getNftAvatarImageUrl({ image_url: "   " })).toBeUndefined();
      expect(getNftAvatarImageUrl({ image_url: 42 })).toBeUndefined();
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
