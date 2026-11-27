//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { toHexString } from "@utils/helpers";
import { hexToU8 } from "cbor2/utils";

const HEX_PATTERN = /^[0-9a-fA-F]*$/;
const utf8Decoder = new TextDecoder("utf-8", { fatal: true });

export interface DecodedDestinationTagMemo {
  destinationTag: string;
  message: string;
}

export function tryDecodeUtf8(bytes: Uint8Array): string | null {
  try {
    return utf8Decoder.decode(bytes);
  } catch {
    return null;
  }
}

export function decodeDestinationTagAndBytes(hexPayload: string): DecodedDestinationTagMemo | null {
  if (hexPayload.length === 0 || hexPayload.length % 2 !== 0 || !HEX_PATTERN.test(hexPayload)) {
    return null;
  }

  const bytes = hexToU8(hexPayload);
  const tagLength = bytes[0];
  if (bytes.length < 1 + tagLength) {
    return null;
  }

  const tagBytes = bytes.slice(1, 1 + tagLength);
  const messageBytes = bytes.slice(1 + tagLength);

  const destinationTag = tagBytes.length > 0 ? (tryDecodeUtf8(tagBytes) ?? toHexString(tagBytes)) : "";
  const message = messageBytes.length > 0 ? (tryDecodeUtf8(messageBytes) ?? toHexString(messageBytes)) : "";

  return { destinationTag, message };
}
