// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useMutation, useQuery } from "@tanstack/react-query";
import { nftList, nftTransfer } from "../../utils/json_rpc";
import { ApiError } from "../helpers/types";
import { TransferNftRequest } from "@tari-project/typescript-bindings";
import queryClient from "../queryClient";
import type { ComponentAddressOrName } from "@tari-project/typescript-bindings/dist";

export interface ListAccountNftsReq {
  account: ComponentAddressOrName;
}

export const useListNfts = (request: ListAccountNftsReq) => {
  return useQuery({
    queryKey: ["list_nfts"],
    queryFn: async () => {
      const limit = 10;
      let offset = 0;
      let nfts = await nftList({
        account: request.account,
        limit: limit,
        offset: offset,
      });
      let result = nfts.nfts;
      while (nfts.nfts.length > 0) {
        offset += limit;
        nfts = await nftList({
          account: request.account,
          limit: 1,
          offset: offset,
        });
        result = result.concat(nfts.nfts);
      }
      return result;
    },
    retry: true,
    retryDelay: 200,
  });
};

export const useNftsTransfer = (request: TransferNftRequest) => {
  return useMutation(
    () => {
      return nftTransfer(request);
    },
    {
      onError: (error: ApiError) => {
        error;
      },
      onSettled: () => {
        queryClient.invalidateQueries(["nfts"]);
      },
    },
  );
};
