//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { FeeReceipt } from "@tari-project/ootle-ts-bindings";

export interface FeeReceiptFigures {
  totalFeesPaid: bigint;
  totalFeesCharged: bigint;
  totalRefunded: bigint;
  overcharge: bigint;
}

function unsignedSaturatingSub(value: bigint): bigint {
  return value < 0n ? 0n : value;
}

/**
 * `finalFee`, when set, is the amount collected after refunds. The refund is the unspent part of
 * `total_fee_payment`: `payment - charged - overcharge`.
 *
 * The receipt fields are typed as `bigint`, but the JSON-RPC client decodes them as plain numbers at
 * runtime, so every field is converted with `BigInt(...)` before doing arithmetic.
 */
export function feeReceiptFigures(receipt: FeeReceipt, finalFee?: bigint | number | null): FeeReceiptFigures {
  const totalFeesCharged = Object.values(receipt.cost_breakdown?.breakdown ?? {}).reduce(
    (sum, value) => sum + BigInt(value ?? 0),
    0n,
  );
  const totalFeesPaid = finalFee != null ? BigInt(finalFee) : BigInt(receipt.total_fees_paid);
  const payment = BigInt(receipt.total_fee_payment);
  const statedOvercharge = BigInt(receipt.total_fee_overcharge);
  const totalRefunded = unsignedSaturatingSub(payment - totalFeesCharged - statedOvercharge);
  const overcharge = unsignedSaturatingSub(payment - totalFeesCharged - totalRefunded);
  return { totalFeesPaid, totalFeesCharged, totalRefunded, overcharge };
}
