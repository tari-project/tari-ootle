//  Copyright 2022. The Tari Project
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

import { useQuery } from "@tanstack/react-query";
import type { IndexerGetTransactionResultResponse } from "@tari-project/ootle-ts-bindings";
import {
  listRecentTransactions,
  getTransaction,
  getTransactionResult,
} from "../../utils/api";

// A settled result is `{ Finalized: {...} }` or `{ Rejected: {...} }`; a pending one is the
// string "Pending". Both settled states are terminal.
const isSettledResult = (data: IndexerGetTransactionResultResponse | undefined): boolean =>
  data?.result != null &&
  typeof data.result === "object" &&
  ("Finalized" in data.result || "Rejected" in data.result);

interface UseListRecentTransactionsProps {
  last_id: string | null;
  limit: number;
}

export const useListRecentTransactions = ({
                                            last_id,
                                            limit,
                                          }: UseListRecentTransactionsProps) => {
  return useQuery({
    queryKey: ["recent_transactions"],
    queryFn: () => {
      return listRecentTransactions({ last_id, limit });
    },
    refetchInterval: 30 * 1000,
  });
};

export const useGetTransactionResult = (transaction_id: string) => {
  return useQuery({
    queryKey: ["transaction_result", transaction_id],
    queryFn: () => getTransactionResult({ transaction_id }),
    enabled: !!transaction_id,
    // Once a transaction is finalized (Accept/Abort) or rejected its result is immutable, so treat
    // it as permanently fresh — React Query then never auto-refetches it (no polling, window-focus
    // or remount refresh). Pending results keep the default so they still update.
    staleTime: (query) => (isSettledResult(query.state.data) ? Infinity : 0),
  });
};

export const useGetTransaction = (transaction_id: string, enabled: boolean) => {
  return useQuery({
    queryKey: ["transaction", transaction_id],
    queryFn: () => getTransaction(transaction_id),
    enabled: enabled && !!transaction_id,
    // The transaction body never changes, so never auto-refetch it.
    staleTime: Infinity,
  });
};
