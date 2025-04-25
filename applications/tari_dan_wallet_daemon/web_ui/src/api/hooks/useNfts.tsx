// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {useMutation, useQuery} from "@tanstack/react-query";
import {nftList, nftTransfer} from "../../utils/json_rpc";
import {ApiError} from "../helpers/types";
import {ListAccountNftRequest} from "@tari-project/typescript-bindings";
import queryClient from "../queryClient";
import {TransferNftRequest} from "@tari-project/typescript-bindings/dist";

export const useListNfts = (request: ListAccountNftRequest) => {
    return useQuery({
        queryKey: ["list_nfts"],
        queryFn: () => {
            return nftList(request);
        },
        onError: (error: ApiError) => {
            error;
        },
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