//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { ApiError } from "@api/helpers/types";
import queryClient from "@api/queryClient";
import { useMutation, useQuery } from "@tanstack/react-query";
import type {
  AuthCreateApiKeyRequest,
  AuthCreateApiKeyResponse,
  AuthRevokeApiKeyRequest,
} from "@tari-project/ootle-ts-bindings";
import { authCreateApiKey, authListApiKeys, authRevokeApiKey } from "@utils/json_rpc";

const API_KEYS_LIST_QUERY_KEY = ["api_keys_list"];

export const useListApiKeys = () => {
  return useQuery({
    queryKey: API_KEYS_LIST_QUERY_KEY,
    queryFn: () => authListApiKeys({}),
  });
};

export const useCreateApiKey = (
  onSuccess: (response: AuthCreateApiKeyResponse) => void,
) => {
  return useMutation({
    mutationFn: (request: AuthCreateApiKeyRequest) => authCreateApiKey(request),
    onSuccess,
    onError: (error: ApiError) => {
      console.error("authCreateApiKey failed", error);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: API_KEYS_LIST_QUERY_KEY });
    },
  });
};

export const useRevokeApiKey = () => {
  return useMutation({
    mutationFn: (request: AuthRevokeApiKeyRequest) => authRevokeApiKey(request),
    onError: (error: ApiError) => {
      console.error("authRevokeApiKey failed", error);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: API_KEYS_LIST_QUERY_KEY });
    },
  });
};
