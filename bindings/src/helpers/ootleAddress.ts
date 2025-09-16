//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { OotleAddress } from "../types/OotleAddress";
import { Network } from "../types/Network";
import bs58 from "bs58";
import { NetworkByte } from "./NetworkByte";

export const BASE58_ENCODED_LENGTH = 89;

export type DecodedOotleAddress = {
  network: Network;
  accountPublicKey: string;
  viewOnlyKey: string;
};

export function decodeOotleAddress(address: OotleAddress): DecodedOotleAddress {
  const decoded = bs58.decode(address);
  if (decoded.length !== 65) {
    throw new Error(`Invalid Ootle address length: ${decoded.length}, expected 65`);
  }

  const networkByte = decoded[0];
  const accountPublicKey = buf2hex(decoded.slice(1, 33));
  const viewOnlyKey = buf2hex(decoded.slice(33, 65));

  let network: Network;
  switch (networkByte) {
    case NetworkByte.MainNet:
      network = "mainnet";
      break;
    case NetworkByte.StageNet:
      network = "stagenet";
      break;
    case NetworkByte.NextNet:
      network = "nextnet";
      break;
    case NetworkByte.LocalNet:
      network = "localnet";
      break;
    case NetworkByte.Igor:
      network = "igor";
      break;
    case NetworkByte.Esmeralda:
      network = "esmeralda";
      break;

    default:
      throw new Error("Invalid network byte in Ootle address");
  }

  return {
    network,
    accountPublicKey,
    viewOnlyKey,
  };
}

export function encodeOotleAddress(parsed: DecodedOotleAddress): OotleAddress {
  let networkByte: number;
  switch (parsed.network) {
    case "mainnet":
      networkByte = NetworkByte.MainNet;
      break;
    case "stagenet":
      networkByte = NetworkByte.StageNet;
      break;
    case "nextnet":
      networkByte = NetworkByte.NextNet;
      break;
    case "localnet":
      networkByte = NetworkByte.LocalNet;
      break;
    case "igor":
      networkByte = NetworkByte.Igor;
      break;
    case "esmeralda":
      networkByte = NetworkByte.Esmeralda;
      break;
    default:
      throw new Error("Invalid network");
  }

  if (parsed.accountPublicKey.length !== 64 || parsed.viewOnlyKey.length !== 64) {
    throw new Error("Public keys must be 32 bytes (64 hex characters) long");
  }

  const accountPubKeyBytes = hex2buf(parsed.accountPublicKey);
  const viewOnlyKeyBytes = hex2buf(parsed.viewOnlyKey);

  const addressBytes = new Uint8Array(1 + accountPubKeyBytes.length + viewOnlyKeyBytes.length);
  addressBytes[0] = networkByte;
  addressBytes.set(accountPubKeyBytes, 1);
  addressBytes.set(viewOnlyKeyBytes, 1 + accountPubKeyBytes.length);

  return bs58.encode(addressBytes) as OotleAddress;
}

const BASE58_RE = /^[123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz]+$/;

function isBase58(s: string) {
  return s.length && BASE58_RE.test(s);
}

const HEX = /[\da-fA-F]/i;

function buf2hex(buffer: Uint8Array) {
  return [...buffer].map((x) => x.toString(16).padStart(2, "0")).join("");
}

function hex2buf(string: string): Uint8Array<ArrayBufferLike> {
  if ("fromHex" in Uint8Array && typeof Uint8Array.fromHex === "function") {
    return Uint8Array.fromHex(string);
  }

  const stringLength = string.length;
  if (stringLength % 2 !== 0) throw new SyntaxError("String should be an even number of characters");
  const maxLength = stringLength / 2;
  const bytes = new Uint8Array(maxLength);
  let read = 0;
  let written = 0;
  while (written < maxLength) {
    const hexits = string.slice(read, (read += 2));
    if (!HEX.exec(hexits)) throw new SyntaxError("String should only contain hex characters");
    bytes[written++] = parseInt(hexits, 16);
  }
  return bytes;
}

export function validateOotleAddress(address: string): boolean {
  if (!address || typeof address !== "string") {
    return false;
  }
  // Trim whitespace for consistent validation
  const trimmedAddress = address.trim();
  if (trimmedAddress.length != BASE58_ENCODED_LENGTH) {
    console.debug(
      `validateOotleAddress: Invalid length ${trimmedAddress.length}, expected ${BASE58_ENCODED_LENGTH} for address ${address}`,
    );
    return false;
  }

  if (!isBase58(address)) {
    console.debug(`validateOotleAddress: Address ${address} contains invalid Base58 characters`);
    return false;
  }

  try {
    decodeOotleAddress(address);
  } catch (e) {
    console.error("validateOotleAddress: decode error:", e, address);
    return false;
  }

  return true;
}
