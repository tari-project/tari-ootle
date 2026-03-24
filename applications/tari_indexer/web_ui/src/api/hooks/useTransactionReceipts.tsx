//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import { useQuery } from "@tanstack/react-query";
import type { TransactionReceiptAddress } from "@tari-project/ootle-ts-bindings";
import { listTransactionReceipts, getTransactionReceipt } from "../../utils/api";

export const useListTransactionReceipts = (limit: number) => {
  return useQuery({
    queryKey: ["transaction_receipts", limit],
    queryFn: () => listTransactionReceipts({ ordering: "Descending", limit }),
    refetchInterval: 30 * 1000,
  });
};

export const useGetTransactionReceipt = (address: TransactionReceiptAddress) => {
  return useQuery({
    queryKey: ["transaction_receipt", address],
    queryFn: () => getTransactionReceipt(address),
    enabled: !!address,
  });
};
