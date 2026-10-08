//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! React Query hooks for approving signing requests: requests for this wallet to
//! co-sign a transaction that someone else seals. Pending requests poll
//! quickly, since they wait on the person viewing the page; the full history
//! refreshes slowly.

import { ApiError } from "@api/helpers/types";
import queryClient from "@api/queryClient";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { SigningRequestDecisionRequest, SigningRequestEffectiveStatus } from "@tari-project/ootle-ts-bindings";
import { signingRequestsApprove, signingRequestsList, signingRequestsReject } from "@utils/json_rpc";

const SIGNING_REQUESTS_LIST_QUERY_KEY = ["signing_requests_list"];

const PENDING_POLL_INTERVAL_MS = 5000;
const HISTORY_POLL_INTERVAL_MS = 60000;

export const useListPendingSigningRequests = () => useListQuery("Pending", PENDING_POLL_INTERVAL_MS);

export const useListAllSigningRequests = () => useListQuery(null, HISTORY_POLL_INTERVAL_MS);

const useListQuery = (status: SigningRequestEffectiveStatus | null, refetchInterval: number) =>
  useQuery({
    queryKey: [...SIGNING_REQUESTS_LIST_QUERY_KEY, status],
    queryFn: () => signingRequestsList({ status }),
    refetchInterval,
  });

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
