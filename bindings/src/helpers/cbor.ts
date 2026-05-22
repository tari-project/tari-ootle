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
 * Recognise the Tari CBOR tag set and render known byte-payload tags as canonical address
 * strings (e.g. tag 132 → `vault_<hex>`). Falls through to a `{ tag, value }` envelope for
 * unknown tags so callers can still see them in the JSON output.
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
      return bytesValueToAddressString("nft", value);
    case TariTypeTag.ClaimedOutputTombstoneAddress:
      return bytesValueToAddressString("tombstone", value);
    case TariTypeTag.TemplateAddress:
      return bytesValueToAddressString("template", value);
    case TariTypeTag.Utxo:
      return bytesValueToAddressString("utxo", value);
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

function bytesValueToAddressString(type: string, value: any): string {
  let hex: string;
  if (Array.isArray(value)) {
    hex = value.map((b: number) => ("0" + (b & 0xff).toString(16)).slice(-2)).join("");
  } else if (typeof value === "string") {
    hex = value;
  } else {
    return JSON.stringify(value);
  }
  return `${type}_${hex}`;
}
