// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

// JSON-side decoder for `tari_bor::Value`.
//
// The minicbor-based encoding (see `crates/tari_bor/src/value_serde.rs`) emits values as natural
// JSON for primitives, text-keyed maps and arrays, and uses `@cbor` sentinel objects otherwise:
//   - `{ "@cbor": "bytes", "hex": "..." }`
//   - `{ "@cbor": "int",   "value": "..." }`   (integers outside i64/u64)
//   - `{ "@cbor": "map",   "entries": [[k, v], ...] }`
//   - `{ "@cbor": "tag",   "tag": N, "value": v }`
//
// The decoder in this file is generic over the meaning of CBOR tags — consumers plug in a
// `CborTagHandler` to translate domain-specific tags. The exported `convertCborValue` is the
// Tari-flavoured convenience wrapper that uses `tariCborTagHandler` to render known address
// tags as canonical `<type>_<hex>` strings.

import { TariTypeTag } from "./tariTypeTag";

export const CBOR_SENTINEL_KEY = "@cbor";
export const CBOR_SENTINEL_BYTES = "bytes";
export const CBOR_SENTINEL_INT = "int";
export const CBOR_SENTINEL_MAP = "map";
export const CBOR_SENTINEL_TAG = "tag";

/**
 * Decode `BinaryTag(N, value)` into a domain-specific representation. Receives the tag number
 * and the *already-decoded* tag payload. Return whatever you want callers to see — the decoder
 * does not further transform the result. Throw or return `value` for tags you don't recognise.
 */
export type CborTagHandler = (tag: number, value: any) => any;

export interface DecodeCborJsonOptions {
  /** Plug-in tag handler. Defaults to a passthrough that returns `{ tag, value }`. */
  onTag?: CborTagHandler;
}

/**
 * Walk JSON produced by the sentinel encoding of `tari_bor::Value` and return native JS:
 *
 * - primitives, arrays, and text-keyed objects pass through (after recursive decoding);
 * - `{ "@cbor": "bytes", hex }` → `number[]` of byte values;
 * - `{ "@cbor": "int", value }` → `number` when safe, otherwise `BigInt`;
 * - `{ "@cbor": "map", entries }` → plain object (keys coerced via `String(decoded)`);
 * - `{ "@cbor": "tag", tag, value }` → whatever `opts.onTag` returns.
 *
 * Decoding is shallow-by-default for tags: `onTag` receives the *decoded* `value`, so it can
 * inspect it without recursing again.
 */
export function decodeCborJson(input: any, opts?: DecodeCborJsonOptions): any {
  const onTag = opts?.onTag ?? defaultTagHandler;
  return walk(input, onTag);
}

/**
 * Tari-flavoured `convertCborValue`. Identical to `decodeCborJson(input, { onTag: tariCborTagHandler })`.
 */
export function convertCborValue(input: any): any {
  return decodeCborJson(input, { onTag: tariCborTagHandler });
}

/**
 * Recognise the Tari CBOR tag set and render known tagged types as canonical address
 * strings (e.g. tag 132 → `vault_<hex>`, tag 130 → `nft_<resource>_<id>`, tag 141 → `utxo_<resource>_<id>`).
 * Falls through to a `{ tag, value }` envelope for unknown or malformed tags so callers can still see them in the JSON output.
 */
export function tariCborTagHandler(tag: number, value: any): any {
  switch (tag) {
    case TariTypeTag.VaultId:
      return bytesValueToAddressString("vault", value);
    case TariTypeTag.ComponentAddress:
      return bytesValueToAddressString("component", value);
    case TariTypeTag.ResourceAddress:
      return bytesValueToAddressString("resource", value);
    case TariTypeTag.NonFungibleAddress:
      return decodeNonFungibleAddress(value, tag);
    case TariTypeTag.ClaimedOutputTombstoneAddress:
      return bytesValueToAddressString("tombstone", value);
    case TariTypeTag.TemplateAddress:
      return bytesValueToAddressString("template", value);
    case TariTypeTag.ValidatorNodeFeePool:
      return bytesValueToAddressString("vnfp", value);
    case TariTypeTag.Utxo:
      return decodeUtxoAddress(value, tag);
    case TariTypeTag.TransactionReceipt:
      return bytesValueToAddressString("txreceipt", value);
    case TariTypeTag.Metadata:
      return value;
    default:
      return { tag, value };
  }
}

function defaultTagHandler(tag: number, value: any): any {
  return { tag, value };
}

function walk(value: any, onTag: CborTagHandler): any {
  if (value === null || value === undefined) {
    return null;
  }

  if (typeof value !== "object") {
    return value;
  }

  if (Array.isArray(value)) {
    return value.map((v) => walk(v, onTag));
  }

  if (CBOR_SENTINEL_KEY in value) {
    return walkSentinel(value, onTag);
  }

  const result: Record<string, any> = {};
  for (const [k, v] of Object.entries(value)) {
    result[k] = walk(v, onTag);
  }
  return result;
}

function walkSentinel(value: Record<string, any>, onTag: CborTagHandler): any {
  switch (value[CBOR_SENTINEL_KEY]) {
    case CBOR_SENTINEL_BYTES:
      return hexToBytes(value.hex);
    case CBOR_SENTINEL_INT: {
      const big = BigInt(value.value);
      const num = Number(big);
      return Number.isSafeInteger(num) && BigInt(num) === big ? num : big;
    }
    case CBOR_SENTINEL_MAP: {
      const result: Record<string, any> = {};
      for (const [k, v] of value.entries) {
        result[String(walk(k, onTag))] = walk(v, onTag);
      }
      return result;
    }
    case CBOR_SENTINEL_TAG:
      return onTag(value.tag, walk(value.value, onTag));
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

function isByteArray(value: unknown): value is number[] {
  return Array.isArray(value) && value.every((b) => typeof b === "number" && Number.isInteger(b) && b >= 0 && b <= 255);
}

function bytesToHex(bytes: readonly number[]): string {
  return bytes.map((b) => ("0" + (b & 0xff).toString(16)).slice(-2)).join("");
}

function extractHexWithOptionalPrefix(value: unknown, prefix: string): string | null {
  if (typeof value === "string") {
    return value.startsWith(prefix) ? value.slice(prefix.length) : value;
  }
  if (isByteArray(value)) {
    return bytesToHex(value);
  }
  return null;
}

function decodeNonFungibleId(raw: unknown): string | null {
  if (!Array.isArray(raw) || raw.length !== 2) {
    return null;
  }
  const [variant, fields] = raw;
  if (!Array.isArray(fields) || fields.length !== 1) {
    return null;
  }
  const inner = fields[0];
  switch (variant) {
    case 0:
      if (isByteArray(inner)) {
        return `uuid_${bytesToHex(inner)}`;
      }
      return typeof inner === "string" ? `uuid_${inner}` : null;
    case 1:
      return typeof inner === "string" ? `str_${inner}` : null;
    case 2:
      return typeof inner === "number" && Number.isInteger(inner) && inner >= 0 ? `u32_${inner}` : null;
    case 3:
      return (typeof inner === "number" && Number.isInteger(inner) && inner >= 0) ||
        (typeof inner === "bigint" && inner >= 0n)
        ? `u64_${inner.toString()}`
        : null;
    default:
      return null;
  }
}

function decodeNonFungibleAddress(value: any, tag: number): any {
  if (Array.isArray(value) && value.length === 2) {
    const resourceHex = extractHexWithOptionalPrefix(value[0], "resource_");
    const idStr = decodeNonFungibleId(value[1]);
    if (resourceHex !== null && idStr !== null) {
      return `nft_${resourceHex}_${idStr}`;
    }
    return { tag, value };
  }
  if (isByteArray(value) || typeof value === "string") {
    return bytesValueToAddressString("nft", value);
  }
  return { tag, value };
}

function decodeUtxoAddress(value: any, tag: number): any {
  if (Array.isArray(value) && value.length === 2) {
    const resourceHex = extractHexWithOptionalPrefix(value[0], "resource_");
    const idHex = extractHexWithOptionalPrefix(value[1], "utxo_");
    if (resourceHex !== null && idHex !== null) {
      return `utxo_${resourceHex}_${idHex}`;
    }
    return { tag, value };
  }
  if (isByteArray(value) || typeof value === "string") {
    return bytesValueToAddressString("utxo", value);
  }
  return { tag, value };
}

function bytesValueToAddressString(type: string, value: any): string {
  let hex: string;
  if (isByteArray(value)) {
    hex = bytesToHex(value);
  } else if (typeof value === "string") {
    hex = value;
  } else {
    return JSON.stringify(value);
  }
  return `${type}_${hex}`;
}
