//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! React Query hooks for the transaction-request approval flow (issue #2343).
//!
//! Approving is a separately-permissioned act from creating, so this UI is the
//! approver's side: it lists what is waiting and authorises or refuses it.
//!
//! The list polls. The daemon does emit a `TransactionRequestCreated` event,
//! but its notifier is a broadcast with no delivery guarantee and does not
//! survive a restart, so a refetch is the only honest source of truth for
//! "what is waiting for me right now".

import { ApiError } from "@api/helpers/types";
import queryClient from "@api/queryClient";
import { useMutation, useQuery } from "@tanstack/react-query";
import type {
  TransactionRequestDecisionRequest,
  TransactionRequestSubmitRequest,
} from "@tari-project/ootle-ts-bindings";
import {
  transactionRequestsApprove,
  transactionRequestsList,
  transactionRequestsReject,
  transactionRequestsSubmit,
} from "@utils/json_rpc";

const TRANSACTION_REQUESTS_LIST_QUERY_KEY = ["transaction_requests_list"];

/// A pending request is only actionable until it expires, so the list must go
/// stale on its own rather than wait for the user to navigate.
const POLL_INTERVAL_MS = 5000;

export const useListTransactionRequests = () => {
  return useQuery({
    queryKey: TRANSACTION_REQUESTS_LIST_QUERY_KEY,
    queryFn: () => transactionRequestsList({ status: null }),
    refetchInterval: POLL_INTERVAL_MS,
  });
};

/// Returning the invalidate promise keeps the mutation `isPending` until the
/// refetched list actually reflects the new state. The button's spinner must
/// not stop while the card still shows the old status — a user reads that gap
/// as "the click didn't take" and clicks again, racing whatever principal acts
/// on the request next (this also applies when the RPC *fails* because the
/// state already moved on: the refetch is what shows them why).
const refetchList = () => queryClient.invalidateQueries({ queryKey: TRANSACTION_REQUESTS_LIST_QUERY_KEY });

export const useApproveTransactionRequest = () => {
  return useMutation({
    mutationFn: (request: TransactionRequestDecisionRequest) => transactionRequestsApprove(request),
    onError: (error: ApiError) => {
      console.error("transactionRequestsApprove failed", error);
    },
    onSettled: refetchList,
  });
};

export const useRejectTransactionRequest = () => {
  return useMutation({
    mutationFn: (request: TransactionRequestDecisionRequest) => transactionRequestsReject(request),
    onError: (error: ApiError) => {
      console.error("transactionRequestsReject failed", error);
    },
    onSettled: refetchList,
  });
};

/// Submitting is a distinct permission from approving, even though the web UI
/// typically holds both. Keeping it a separate action (rather than folding it
/// into approve) means the two stay independently grantable, and an approved
/// request that fails to submit stays approved rather than being lost.
export const useSubmitTransactionRequest = () => {
  return useMutation({
    mutationFn: (request: TransactionRequestSubmitRequest) => transactionRequestsSubmit(request),
    onError: (error: ApiError) => {
      console.error("transactionRequestsSubmit failed", error);
    },
    onSettled: refetchList,
  });
};
