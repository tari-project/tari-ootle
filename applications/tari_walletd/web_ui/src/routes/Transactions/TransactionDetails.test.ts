// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { getTransactionFilename, serializeTransactionJson } from "./TransactionDetails";

describe("serializeTransactionJson", () => {
  it("serializes BigInt values above Number.MAX_SAFE_INTEGER as decimal strings", () => {
    const payload = {
      final_fee: 9007199254740993n,
      transaction: {
        min_epoch: 10n,
        max_epoch: 9007199254740999n,
      },
      tags: ["ok", 42n],
    };

    expect(JSON.parse(serializeTransactionJson(payload))).toEqual({
      final_fee: "9007199254740993",
      transaction: {
        min_epoch: "10",
        max_epoch: "9007199254740999",
      },
      tags: ["ok", "42"],
    });
  });

  it("pretty-prints standard JSON values with 2-space indentation", () => {
    expect(serializeTransactionJson({ status: "Accepted", fee: 0 })).toBe('{\n  "status": "Accepted",\n  "fee": 0\n}');
  });
});

describe("getTransactionFilename", () => {
  it("uses the trimmed transaction ID when present", () => {
    expect(getTransactionFilename("abc123")).toBe("tx-abc123.json");
    expect(getTransactionFilename("  abc123  ")).toBe("tx-abc123.json");
  });

  it("falls back to tx-unknown_id.json when the ID is missing or blank", () => {
    expect(getTransactionFilename(undefined)).toBe("tx-unknown_id.json");
    expect(getTransactionFilename("")).toBe("tx-unknown_id.json");
    expect(getTransactionFilename("   ")).toBe("tx-unknown_id.json");
  });
});
