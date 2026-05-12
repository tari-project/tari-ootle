//  Copyright 2026 The Tari Project
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

import { ApiError } from "@api/helpers/types";
import queryClient from "@api/queryClient";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { JrpcPermission } from "@tari-project/ootle-ts-bindings";
import { apiKeysCreate, apiKeysList, apiKeysRevoke } from "@utils/json_rpc";

export interface AgentTokenInfo {
  id: string;
  name: string;
  permissions: JrpcPermission[];
  created_at: number;
  expires_at: number | null;
  last_used: number | null;
  revoked: boolean;
}

export interface AgentTokenListResponse {
  keys: AgentTokenInfo[];
}

export interface AgentTokenCreateResponse {
  id: string;
  name: string;
  permissions: JrpcPermission[];
  created_at: number;
  expires_at: number | null;
  key: string;
}

export const useGetAgentTokens = () => {
  return useQuery<AgentTokenListResponse>({
    queryKey: ["agent_tokens"],
    queryFn: () => apiKeysList() as Promise<AgentTokenListResponse>,
  });
};

export const useCreateAgentToken = () => {
  return useMutation<
    AgentTokenCreateResponse,
    ApiError,
    { name: string; permissions: JrpcPermission[]; grantAdmin: boolean }
  >({
    mutationFn: ({ name, permissions, grantAdmin }) =>
      apiKeysCreate(name, permissions, grantAdmin) as Promise<AgentTokenCreateResponse>,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["agent_tokens"] });
    },
  });
};

export const useRevokeAgentToken = () => {
  return useMutation<void, ApiError, string>({
    mutationFn: (id) => apiKeysRevoke(id),
    onError: (error: ApiError) => {
      console.error(error);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["agent_tokens"] });
    },
  });
};
