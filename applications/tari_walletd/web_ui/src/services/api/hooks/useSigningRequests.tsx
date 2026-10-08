//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! React Query hooks for approving signing requests: requests for this wallet to
//! co-sign a transaction that someone else seals. The list polls, as the
//! transaction-request list does.

import { ApiError } from "@api/helpers/types";
import queryClient from "@api/queryClient";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { SigningRequestDecisionRequest } from "@tari-project/ootle-ts-bindings";
import { signingRequestsApprove, signingRequestsList, signingRequestsReject } from "@utils/json_rpc";

const SIGNING_REQUESTS_LIST_QUERY_KEY = ["signing_requests_list"];

const POLL_INTERVAL_MS = 5000;

export const useListSigningRequests = () => {
  return useQuery({
    queryKey: SIGNING_REQUESTS_LIST_QUERY_KEY,
    queryFn: () => signingRequestsList({ status: null }),
    refetchInterval: POLL_INTERVAL_MS,
  });
};

/// The mutation stays pending until the refetched list shows the decision, so
/// the button spinner and the card's status change together.
const refetchList = () => queryClient.invalidateQueries({ queryKey: SIGNING_REQUESTS_LIST_QUERY_KEY });

export const useApproveSigningRequest = () => {
  return useMutation({
    mutationFn: (request: SigningRequestDecisionRequest) => signingRequestsApprove(request),
    onError: (error: ApiError) => {
      console.error("signingRequestsApprove failed", error);
    },
    onSettled: refetchList,
  });
};

export const useRejectSigningRequest = () => {
  return useMutation({
    mutationFn: (request: SigningRequestDecisionRequest) => signingRequestsReject(request),
    onError: (error: ApiError) => {
      console.error("signingRequestsReject failed", error);
    },
    onSettled: refetchList,
  });
};
