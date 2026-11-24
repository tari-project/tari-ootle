//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { hexToU8 } from "cbor2/utils";

const HEX_PATTERN = /^[0-9a-fA-F]*$/;
const utf8Decoder = new TextDecoder("utf-8", { fatal: true });

export interface DecodedDestinationTagMemo {
  destinationTag: string;
  message: string;
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function tryDecodeUtf8(bytes: Uint8Array): string | null {
  try {
    return utf8Decoder.decode(bytes);
  } catch {
    return null;
  }
}

export function formatMemoBytes(bytes: string | Uint8Array | number[]): string {
  if (typeof bytes === "string") {
    return bytes;
  }
  if (bytes instanceof Uint8Array) {
    return bytesToHex(bytes);
  }
  return bytesToHex(Uint8Array.from(bytes));
}

export function decodeDestinationTagAndBytes(hexPayload: string): DecodedDestinationTagMemo | null {
  if (hexPayload.length === 0 || hexPayload.length % 2 !== 0 || !HEX_PATTERN.test(hexPayload)) {
    return null;
  }

  let bytes: Uint8Array;
  try {
    bytes = hexToU8(hexPayload);
  } catch {
    return null;
  }

  if (bytes.length < 1) {
    return null;
  }

  const tagLength = bytes[0];
  if (bytes.length < 1 + tagLength) {
    return null;
  }

  const tagBytes = bytes.slice(1, 1 + tagLength);
  const messageBytes = bytes.slice(1 + tagLength);

  const destinationTag = tagBytes.length > 0 ? (tryDecodeUtf8(tagBytes) ?? bytesToHex(tagBytes)) : "";
  const message = messageBytes.length > 0 ? (tryDecodeUtf8(messageBytes) ?? bytesToHex(messageBytes)) : "";

  return { destinationTag, message };
}
