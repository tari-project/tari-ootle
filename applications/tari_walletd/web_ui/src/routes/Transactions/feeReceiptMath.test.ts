//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { FeeReceipt } from "@tari-project/ootle-ts-bindings";
import { describe, expect, it } from "vitest";
import { feeReceiptFigures } from "./feeReceiptMath";

function receipt(partial: Partial<FeeReceipt> & Pick<FeeReceipt, "total_fee_payment" | "total_fees_paid">): FeeReceipt {
  return {
    total_fee_overcharge: 0n,
    cost_breakdown: { breakdown: {} },
    exhaust_burn: 0n,
    ...partial,
  };
}

describe("feeReceiptFigures", () => {
  it("refunds the unspent payment when final_fee is only what was collected", () => {
    // max_fee 1000, charged 100, the rest returned to the paying vault.
    const figures = feeReceiptFigures(
      receipt({
        total_fee_payment: 1000n,
        total_fees_paid: 100n,
        cost_breakdown: { breakdown: { Initial: 100n } },
      }),
      100,
    );

    expect(figures.totalFeesPaid).toBe(100n);
    expect(figures.totalFeesCharged).toBe(100n);
    expect(figures.totalRefunded).toBe(900n);
    expect(figures.overcharge).toBe(0n);
  });

  it("keeps a non-refundable stealth overcharge and refunds nothing", () => {
    const figures = feeReceiptFigures(
      receipt({
        total_fee_payment: 1000n,
        total_fees_paid: 1000n,
        total_fee_overcharge: 900n,
        cost_breakdown: { breakdown: { Initial: 40n, Storage: 60n } },
      }),
      1000n,
    );

    expect(figures.totalFeesPaid).toBe(1000n);
    expect(figures.totalFeesCharged).toBe(100n);
    expect(figures.totalRefunded).toBe(0n);
    expect(figures.overcharge).toBe(900n);
  });

  it("refunds nothing when the payment did not cover the charges", () => {
    const figures = feeReceiptFigures(
      receipt({
        total_fee_payment: 50n,
        total_fees_paid: 50n,
        cost_breakdown: { breakdown: { Initial: 100n } },
      }),
      50,
    );

    expect(figures.totalFeesPaid).toBe(50n);
    expect(figures.totalFeesCharged).toBe(100n);
    expect(figures.totalRefunded).toBe(0n);
    expect(figures.overcharge).toBe(0n);
  });

  it("uses the receipt's collected amount when final_fee is absent", () => {
    const figures = feeReceiptFigures(
      receipt({
        total_fee_payment: 1000n,
        total_fees_paid: 100n,
        cost_breakdown: { breakdown: { Initial: 100n } },
      }),
      null,
    );

    expect(figures.totalFeesPaid).toBe(100n);
    expect(figures.totalRefunded).toBe(900n);
  });
});
