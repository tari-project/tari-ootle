// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { TariTypeTag } from "@tari-project/ootle-ts-bindings";

// JSON-side decoder for `tari_bor::Value`.
//
// The minicbor-based encoding (see `crates/tari_bor/src/value_serde.rs`) emits values as natural
// JSON for primitives, text-keyed maps and arrays, and uses `@cbor` sentinel objects otherwise:
//   - `{ "@cbor": "bytes", "hex": "..." }`
//   - `{ "@cbor": "int",   "value": "..." }`   (integers outside i64/u64)
//   - `{ "@cbor": "map",   "entries": [[k, v], ...] }`
//   - `{ "@cbor": "tag",   "tag": N, "value": v }`

const SENTINEL_KEY = "@cbor";

export function convertCborValue(value: any): any {
  if (value === null || value === undefined) {
    return null;
  }

  if (typeof value !== "object") {
    return value;
  }

  if (Array.isArray(value)) {
    return value.map(convertCborValue);
  }

  if (SENTINEL_KEY in value) {
    return convertSentinel(value);
  }

  const result: Record<string, any> = {};
  for (const [k, v] of Object.entries(value)) {
    result[k] = convertCborValue(v);
  }
  return result;
}

function convertSentinel(value: Record<string, any>): any {
  switch (value[SENTINEL_KEY]) {
    case "bytes":
      return hexToBytes(value.hex);
    case "int": {
      const big = BigInt(value.value);
      const num = Number(big);
      return Number.isSafeInteger(num) && BigInt(num) === big ? num : big;
    }
    case "map": {
      const result: Record<string, any> = {};
      for (const [k, v] of value.entries) {
        result[String(convertCborValue(k))] = convertCborValue(v);
      }
      return result;
    }
    case "tag":
      return convertTaggedValue(value.tag, value.value);
    default:
      return value;
  }
}

function hexToBytes(hex: string): number[] {
  const out: number[] = [];
  for (let i = 0; i + 1 < hex.length; i += 2) {
    out.push(parseInt(hex.substring(i, i + 2), 16));
  }
  return out;
}

function bytesToAddressString(type: string, value: any): string {
  let hex: string;
  if (value && typeof value === "object" && value[SENTINEL_KEY] === "bytes") {
    hex = value.hex;
  } else if (Array.isArray(value)) {
    hex = value.map((b: number) => ("0" + (b & 0xff).toString(16)).slice(-2)).join("");
  } else if (typeof value === "string") {
    hex = value;
  } else {
    return JSON.stringify(value);
  }
  return `${type}_${hex}`;
}

export function convertTaggedValue(tag: number, value: any): any {
  switch (tag) {
    case TariTypeTag.VaultId:
      return bytesToAddressString("vault", value);
    case TariTypeTag.ComponentAddress:
      return bytesToAddressString("component", value);
    case TariTypeTag.ResourceAddress:
      return bytesToAddressString("resource", value);
    case TariTypeTag.NonFungibleAddress:
      return bytesToAddressString("non_fungible", value);
    case TariTypeTag.ClaimedOutputTombstoneAddress:
      return bytesToAddressString("tombstone", value);
    case TariTypeTag.TemplateAddress:
      return bytesToAddressString("template", value);
    case TariTypeTag.Utxo:
      return bytesToAddressString("utxo", value);
    case TariTypeTag.TransactionReceipt:
      return bytesToAddressString("txreceipt", value);
    case TariTypeTag.Metadata:
      return convertCborValue(value);
    default:
      return convertCborValue(value);
  }
}
