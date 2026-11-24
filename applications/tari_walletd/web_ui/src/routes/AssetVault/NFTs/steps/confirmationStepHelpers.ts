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

import type { NonFungibleId, NonFungibleToken } from "@tari-project/ootle-ts-bindings";
import { nftIdToString } from "@utils/helpers";

export const FALLBACK_NFT_IMG =
  "data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iODAiIGhlaWdodD0iODAiIHZpZXdCb3g9IjAgMCA4MCA4MCIgZmlsbD0ibm9uZSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj4KPHJlY3Qgd2lkdGg9IjgwIiBoZWlnaHQ9IjgwIiBmaWxsPSIjRjVGNUY1Ii8+CjxwYXRoIGQ9Ik0zMCAyNUg1MFY1NUgzMFYyNVoiIGZpbGw9IiNERERERUREIi8+CjxwYXRoIGQ9Ik0zNiAzMUg0NFY0M0gzNlYzMVoiIGZpbGw9IiNCQkJCQkIiLz4KPHR1eHQgeD0iNDAiIHk9IjUyIiBmb250LWZhbWlseT0iQXJpYWwiIGZvbnQtc2l6ZT0iOCIgZmlsbD0iIzk5OTk5OSIgdGV4dC1hbmNob3I9Im1pZGRsZSI+TkZUPC90ZXh0Pgo8L3N2Zz4K";

const UNSIGNED_INT_PATTERN = /^\d+$/;

export function parseMaxFeeAmount(maxFee: string): bigint | null {
  const trimmed = maxFee.trim();
  if (!UNSIGNED_INT_PATTERN.test(trimmed)) {
    return null;
  }
  return BigInt(trimmed);
}

export function getNftAvatarImageUrl(mutableData: unknown): string | undefined {
  if (mutableData === null || typeof mutableData !== "object" || Array.isArray(mutableData)) {
    return undefined;
  }
  const imageUrl = (mutableData as Record<string, unknown>).image_url;
  if (typeof imageUrl !== "string") {
    return undefined;
  }
  const trimmed = imageUrl.trim();
  return trimmed.length > 0 ? trimmed : undefined;
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
