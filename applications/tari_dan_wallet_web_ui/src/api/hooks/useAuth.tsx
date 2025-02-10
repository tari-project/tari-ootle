// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {useQuery} from "@tanstack/react-query";
import {authGetMethod} from "../../utils/json_rpc";
import {ApiError} from "../helpers/types";

export const useAuthMethod = () => {
    return useQuery({
        queryKey: ["keys_list"],
        queryFn: () => {
            return authGetMethod();
        },
        onError: (error: ApiError) => {
            error;
        },
        refetchInterval: 1000,
        retry: true,
    });
};