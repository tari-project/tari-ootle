// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useMutation, useQuery } from "@tanstack/react-query";
import { nftList, nftTransfer } from "@utils/json_rpc";
import { ApiError } from "@api/helpers/types";
import { TransferNftRequest } from "@tari-project/ootle-ts-bindings";
import queryClient from "@api/queryClient";
import { ComponentAddress, ComponentAddressOrName } from "@tari-project/ootle-ts-bindings";

export interface ListAccountNftsReq {
  account: ComponentAddressOrName | null;
  enabled?: boolean;
}

export const useNFTsList = (account: ComponentAddress, offset: number, limit: number) => {
  return useQuery({
    queryKey: ["nfts_list", account, offset, limit],
    queryFn: () => nftList({ account: { ComponentAddress: account }, offset, limit }),
    enabled: !!account,
    staleTime: 30000,
    gcTime: 60000,
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false,
    retry: 2,
    retryDelay: (attemptIndex) => Math.min(2000 * 2 ** attemptIndex, 8000),
  });
};

export const useNftsTransfer = (request: TransferNftRequest) => {
  return useMutation({
    mutationFn: () => {
      return nftTransfer(request);
    },
    onError: (error: ApiError) => {
      error;
    },
    onSettled: () => {
      // Invalidate all NFT-related queries to trigger fresh fetch
      queryClient.invalidateQueries({
        predicate: (query) => {
          const key = query.queryKey[0];
          return typeof key === "string" && (key === "nfts" || key === "list_nfts" || key === "nfts_list");
        },
      });
    },
  });
};
