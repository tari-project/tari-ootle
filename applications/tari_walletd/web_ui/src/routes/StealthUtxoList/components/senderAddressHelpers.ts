// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { decodeOotleAddressOrNull } from "@tari-project/ootle-ts-bindings";

export interface ParsedSenderAddress {
  identityKey: string | null;
  destinationTagText: string | null;
}

export function formatDestinationTag(destinationTag: string | Uint8Array | null | undefined): string | null {
  if (!destinationTag) return null;
  if (typeof destinationTag === "string") return destinationTag.length > 0 ? destinationTag : null;
  if (destinationTag.length === 0) return null;
  // Non-UTF-8 bytes: show as hex so the user can still copy/identify the value.
  return Array.from(destinationTag)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export function parseSenderAddress(address: string): ParsedSenderAddress {
  const decoded = decodeOotleAddressOrNull(address);
  if (!decoded) {
    return { identityKey: null, destinationTagText: null };
  }
  return {
    identityKey: `${decoded.accountPublicKey}:${decoded.viewOnlyKey}`,
    destinationTagText: formatDestinationTag(decoded.destinationTag ?? null),
  };
}

// Identity key for "is this the same account?" comparisons. We drop the optional destination tag so an incoming
// SenderAddress with a destination tag still matches the wallet's own account address (which has no tag) or an
// address-book entry stored without one.
export function identityKey(address: string): string | null {
  return parseSenderAddress(address).identityKey;
}

export function formatAddressBookAddError(error: unknown): string {
  const err = error as { cause?: { message?: string }; message?: string } | null | undefined;
  const msg = err?.cause?.message || err?.message || "Failed to add contact";
  return msg.includes("DuplicateName") ? "An entry with this name already exists" : msg;
}
