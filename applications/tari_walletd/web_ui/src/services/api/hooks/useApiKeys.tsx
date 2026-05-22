//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! React Query hooks for the admin-only API key management endpoints
//! introduced by issue #1957.
//!
//! Mutations invalidate the `api_keys_list` query so the UI re-renders
//! immediately after create / revoke. The raw key material that
//! `auth.create_api_key` returns is NEVER stored anywhere by these
//! hooks — it's surfaced once via the mutation result and the caller
//! component is responsible for displaying it (and warning the user it
//! will not be shown again).

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
