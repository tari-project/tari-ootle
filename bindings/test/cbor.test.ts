// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { convertCborValue, TariTypeTag } from "../src";

const RESOURCE_HEX = "cc".repeat(32);
const RESOURCE_TAG = {
  "@cbor": "tag",
  "tag": TariTypeTag.ResourceAddress,
  "value": { "@cbor": "bytes", "hex": RESOURCE_HEX },
};

describe("convertCborValue", () => {
  it("decodes NonFungibleAddress with string id", () => {
    const input = {
      "@cbor": "tag",
      "tag": TariTypeTag.NonFungibleAddress,
      "value": [RESOURCE_TAG, [1, ["nft-1"]]],
    };
    expect(convertCborValue(input)).toBe(`nft_${RESOURCE_HEX}_str_nft-1`);
  });

  it("decodes NonFungibleAddress with uuid id", () => {
    const uuidHex = "5a".repeat(32);
    const input = {
      "@cbor": "tag",
      "tag": TariTypeTag.NonFungibleAddress,
      "value": [RESOURCE_TAG, [0, [{ "@cbor": "bytes", "hex": uuidHex }]]],
    };
    expect(convertCborValue(input)).toBe(`nft_${RESOURCE_HEX}_uuid_${uuidHex}`);
  });

  it("decodes NonFungibleAddress with u32 and u64 ids", () => {
    const u32Input = {
      "@cbor": "tag",
      "tag": TariTypeTag.NonFungibleAddress,
      "value": [RESOURCE_TAG, [2, [7]]],
    };
    expect(convertCborValue(u32Input)).toBe(`nft_${RESOURCE_HEX}_u32_7`);

    // `tari_bor::Value` serializes u64 values as plain JSON numbers.
    const u64Input = {
      "@cbor": "tag",
      "tag": TariTypeTag.NonFungibleAddress,
      "value": [RESOURCE_TAG, [3, [17179869189]]],
    };
    expect(convertCborValue(u64Input)).toBe(`nft_${RESOURCE_HEX}_u64_17179869189`);
  });

  it("decodes Utxo address from resource tag and commitment bytes", () => {
    const idHex = "56".repeat(32);
    const input = {
      "@cbor": "tag",
      "tag": TariTypeTag.Utxo,
      "value": [RESOURCE_TAG, { "@cbor": "bytes", "hex": idHex }],
    };
    expect(convertCborValue(input)).toBe(`utxo_${RESOURCE_HEX}_${idHex}`);
  });

  it("decodes ValidatorNodeFeePool address tag as vnfp_<hex>", () => {
    const poolHex = "34".repeat(32);
    const input = {
      "@cbor": "tag",
      "tag": TariTypeTag.ValidatorNodeFeePool,
      "value": { "@cbor": "bytes", "hex": poolHex },
    };
    expect(convertCborValue(input)).toBe(`vnfp_${poolHex}`);
  });

  it("falls back to { tag, value } on unexpected payloads", () => {
    const rawHex = "ab".repeat(4);
    expect(
      convertCborValue({
        "@cbor": "tag",
        "tag": TariTypeTag.NonFungibleAddress,
        "value": [RESOURCE_TAG, [99, ["unexpected"]]],
      }),
    ).toEqual({
      tag: TariTypeTag.NonFungibleAddress,
      value: [`resource_${RESOURCE_HEX}`, [99, ["unexpected"]]],
    });
    expect(
      convertCborValue({
        "@cbor": "tag",
        "tag": TariTypeTag.NonFungibleAddress,
        "value": [RESOURCE_TAG, [0, ["not-bytes"]]],
      }),
    ).toEqual({
      tag: TariTypeTag.NonFungibleAddress,
      value: [`resource_${RESOURCE_HEX}`, [0, ["not-bytes"]]],
    });
    expect(
      convertCborValue({
        "@cbor": "tag",
        "tag": TariTypeTag.NonFungibleAddress,
        "value": { "@cbor": "bytes", "hex": rawHex },
      }),
    ).toEqual({
      tag: TariTypeTag.NonFungibleAddress,
      value: [0xab, 0xab, 0xab, 0xab],
    });
    expect(
      convertCborValue({
        "@cbor": "tag",
        "tag": TariTypeTag.Utxo,
        "value": [RESOURCE_TAG, "not-bytes"],
      }),
    ).toEqual({
      tag: TariTypeTag.Utxo,
      value: [`resource_${RESOURCE_HEX}`, "not-bytes"],
    });
  });

  it("decodes Metadata maps containing nested address tags", () => {
    const compHex = "aa".repeat(32);
    const input = {
      "@cbor": "tag",
      "tag": TariTypeTag.Metadata,
      "value": {
        owner: {
          "@cbor": "tag",
          "tag": TariTypeTag.ComponentAddress,
          "value": { "@cbor": "bytes", "hex": compHex },
        },
        nft: {
          "@cbor": "tag",
          "tag": TariTypeTag.NonFungibleAddress,
          "value": [RESOURCE_TAG, [1, ["badge"]]],
        },
      },
    };
    expect(convertCborValue(input)).toEqual({
      owner: `component_${compHex}`,
      nft: `nft_${RESOURCE_HEX}_str_badge`,
    });
  });
});
