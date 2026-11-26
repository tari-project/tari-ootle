//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { NonFungibleId, NonFungibleToken } from "@tari-project/ootle-ts-bindings";
import { nftIdToString } from "@utils/helpers";

const UNSIGNED_INT_PATTERN = /^\d+$/;

export function parseMaxFeeAmount(maxFee: string): bigint | null {
  const trimmed = maxFee.trim();
  if (!UNSIGNED_INT_PATTERN.test(trimmed)) {
    return null;
  }
  return BigInt(trimmed);
}

export function findSelectedNft(
  availableNfts: NonFungibleToken[] | undefined,
  preSelectedNftId: NonFungibleId | undefined,
): NonFungibleToken | undefined {
  if (!preSelectedNftId || !availableNfts?.length) {
    return undefined;
  }
  const targetKey = nftIdToString(preSelectedNftId);
  return availableNfts.find((nft) => nftIdToString(nft.nft_id) === targetKey);
}
