//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { useNftTransferStore } from "@store/nftTransferStore";
import type { NonFungibleToken, ResourceAddress, TransferNftResponse } from "@tari-project/ootle-ts-bindings";
import { beforeEach, describe, expect, it } from "vitest";
import { formatNftTransferRejection, parseNftFeeAmount, resolveSelectedNftTransferState } from "./nftTransferHelpers";

const RESOURCE_A = "resource_0000000000000000000000000000000000000000000000000000000000000001" as ResourceAddress;

function makeNft(id: number, resourceAddress: ResourceAddress = RESOURCE_A): NonFungibleToken {
  return {
    vault_id: "vault_01",
    nft_id: { Uint32: id },
    resource_address: resourceAddress,
    data: {},
    mutable_data: {},
    is_burnt: false,
  } as unknown as NonFungibleToken;
}

describe("resolveSelectedNftTransferState", () => {
  const available = [makeNft(1), makeNft(2)];

  it("clears nfts and maxFee when the last selected NFT is deselected", () => {
    expect(resolveSelectedNftTransferState([], available)).toEqual({
      nfts: [],
      resourceAddress: "",
      maxFee: "",
    });
  });

  it("preserves fallbackResourceAddress when deselecting all NFTs", () => {
    expect(resolveSelectedNftTransferState([], available, RESOURCE_A)).toEqual({
      nfts: [],
      resourceAddress: RESOURCE_A,
      maxFee: "",
    });
  });

  it("resolves selected NFTs, derives resourceAddress, and invalidates maxFee", () => {
    const selected = [JSON.stringify({ Uint32: 1 }), JSON.stringify({ Uint32: 2 })];
    expect(resolveSelectedNftTransferState(selected, available)).toEqual({
      nfts: [{ Uint32: 1 }, { Uint32: 2 }],
      resourceAddress: RESOURCE_A,
      maxFee: "",
    });
  });

  it("ignores malformed or unknown entries safely", () => {
    const selected = ["not-json", JSON.stringify({ Uint32: 99 }), JSON.stringify({ Uint32: 2 })];
    expect(resolveSelectedNftTransferState(selected, available)).toEqual({
      nfts: [{ Uint32: 2 }],
      resourceAddress: RESOURCE_A,
      maxFee: "",
    });
  });
});

describe("parseNftFeeAmount", () => {
  it("parses exact bigint fees including values above Number.MAX_SAFE_INTEGER", () => {
    expect(parseNftFeeAmount("500")).toBe(500n);
    expect(parseNftFeeAmount(" 9007199254740993 ")).toBe(9007199254740993n);
  });

  it("returns 0n for empty or non-integer strings instead of NaN", () => {
    expect(parseNftFeeAmount("")).toBe(0n);
    expect(parseNftFeeAmount("   ")).toBe(0n);
    expect(parseNftFeeAmount("-10")).toBe(0n);
    expect(parseNftFeeAmount("12.5")).toBe(0n);
    expect(parseNftFeeAmount("abc")).toBe(0n);
  });
});

describe("formatNftTransferRejection", () => {
  it("extracts the reject reason from Reject and AcceptFeeRejectRest results", () => {
    const rejected = {
      result: {
        result: {
          Reject: { ExecutionFailure: { code: "E1", message: "insufficient balance" } },
        },
      },
    } as unknown as TransferNftResponse;
    expect(formatNftTransferRejection(rejected)).toBe("ExecutionFailure (E1): insufficient balance");

    const feeOnly = {
      result: {
        result: {
          AcceptFeeRejectRest: [{}, { InsufficientFeesPaid: "needed 500" }],
        },
      },
    } as unknown as TransferNftResponse;
    expect(formatNftTransferRejection(feeOnly)).toBe("InsufficientFeesPaid: needed 500");
  });

  it("falls back to a generic message when no reject reason is present", () => {
    expect(formatNftTransferRejection(undefined)).toBe("Transaction was rejected");
  });
});

describe("useNftTransferStore", () => {
  beforeEach(() => {
    useNftTransferStore.getState().resetState();
  });

  it("clears cached maxFee when targetAccountAddress changes via updateFormValue", () => {
    const store = useNftTransferStore.getState();
    store.setTransferFormState({ maxFee: "420" });
    expect(useNftTransferStore.getState().transferFormState.maxFee).toBe("420");

    useNftTransferStore.getState().updateFormValue("targetAccountAddress", "otl_loc_1test", true);
    expect(useNftTransferStore.getState().transferFormState.targetAccountAddress).toBe("otl_loc_1test");
    expect(useNftTransferStore.getState().transferFormState.maxFee).toBe("");
  });

  it("does not clear maxFee when updateFormValue updates maxFee itself", () => {
    useNftTransferStore.getState().updateFormValue("maxFee", "750");
    expect(useNftTransferStore.getState().transferFormState.maxFee).toBe("750");
  });
});
