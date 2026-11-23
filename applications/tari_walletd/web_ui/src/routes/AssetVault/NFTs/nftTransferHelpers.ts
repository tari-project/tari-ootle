//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import {
  getRejectReasonFromTransactionResult,
  type NonFungibleId,
  type NonFungibleToken,
  rejectReasonToString,
  type ResourceAddress,
  type TransferNftResponse,
} from "@tari-project/ootle-ts-bindings";
import { nftIdToString } from "@utils/helpers";

export interface SelectedNftTransferState {
  nfts: NonFungibleId[];
  resourceAddress: ResourceAddress;
  maxFee: string;
}

/**
 * Resolve multi-select dropdown values (JSON-serialized NonFungibleId strings) into the next
 * NFT transfer form state. Always clears `maxFee` so any prior fee estimate is recalculated.
 */
export function resolveSelectedNftTransferState(
  selectedValues: string[],
  availableNfts: NonFungibleToken[],
  fallbackResourceAddress?: ResourceAddress,
): SelectedNftTransferState {
  const nftsSelected = selectedValues
    .map((raw) => {
      try {
        const nftId = JSON.parse(raw) as NonFungibleId;
        const key = nftIdToString(nftId);
        return availableNfts.find((nft) => nftIdToString(nft.nft_id) === key);
      } catch {
        return undefined;
      }
    })
    .filter((value): value is NonFungibleToken => Boolean(value));

  if (nftsSelected.length === 0) {
    return {
      nfts: [],
      resourceAddress: (fallbackResourceAddress || "") as ResourceAddress,
      maxFee: "",
    };
  }

  return {
    nfts: nftsSelected.map((item) => item.nft_id),
    resourceAddress: nftsSelected[0].resource_address,
    maxFee: "",
  };
}

/**
 * Parse a stored maxFee string into a non-negative bigint without NaN or precision loss.
 */
export function parseNftFeeAmount(maxFee: string): bigint {
  const trimmed = maxFee.trim();
  if (!/^\d+$/.test(trimmed)) {
    return 0n;
  }
  return BigInt(trimmed);
}

/**
 * Format a rejected NFT transfer (or fee estimation) response into a human-readable message.
 */
export function formatNftTransferRejection(result?: TransferNftResponse): string {
  const reason = result ? getRejectReasonFromTransactionResult(result.result.result) : null;
  return reason ? rejectReasonToString(reason) : "Transaction was rejected";
}
