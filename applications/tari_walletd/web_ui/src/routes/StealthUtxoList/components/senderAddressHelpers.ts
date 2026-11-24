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

// Identity key for "is this the same account?" comparisons. We drop the optional destination tag so an incoming
// SenderAddress with a destination tag still matches the wallet's own account address (which has no tag) or an
// address-book entry stored without one.
export function identityKey(address: string): string | null {
  const decoded = decodeOotleAddressOrNull(address);
  if (!decoded) return null;
  return `${decoded.accountPublicKey}:${decoded.viewOnlyKey}`;
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

export function formatAddressBookAddError(error: unknown): string {
  const causeMsg =
    typeof error === "object" &&
    error !== null &&
    "cause" in error &&
    typeof (error as { cause?: { message?: unknown } }).cause?.message === "string"
      ? (error as { cause: { message: string } }).cause.message
      : undefined;
  const directMsg =
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof (error as { message?: unknown }).message === "string"
      ? (error as { message: string }).message
      : undefined;
  const msg = causeMsg || directMsg || "Failed to add contact";
  return msg.includes("DuplicateName") ? "An entry with this name already exists" : msg;
}
