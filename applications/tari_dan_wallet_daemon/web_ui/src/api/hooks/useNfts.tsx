// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {useQuery} from "@tanstack/react-query";
import {nftList} from "../../utils/json_rpc";
import {ApiError} from "../helpers/types";
import {ListAccountNftRequest} from "@tari-project/typescript-bindings";

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