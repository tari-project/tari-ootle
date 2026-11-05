//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! Plain-language descriptions of what a signing request authorises, for the
//! instructions a person can verify by reading them. Everything else falls back
//! to the generic instruction list.

import type { Blobs, Instruction, InstructionArg } from "@tari-project/ootle-ts-bindings";
import { blobBytes, toHexString } from "@utils/helpers";
import { decode } from "cbor2";

/// `BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS`: the same fixed global component on every network.
export const BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS = "component_0104" + "0".repeat(60);

const NETWORK_NAMES: Record<number, string> = {
  0x00: "mainnet",
  0x01: "stagenet",
  0x02: "nextnet",
  0x10: "localnet",
  0x24: "igor",
  0x26: "esmeralda",
};

export function networkName(byte: number): string {
  return NETWORK_NAMES[byte] ?? `unknown network 0x${byte.toString(16).padStart(2, "0")}`;
}

/// The first 8 bytes of the authorization message, grouped for reading aloud:
/// `3F9A-1C07-88D2-B4E1`. Every co-signer of a transaction signs the same
/// message, so the fingerprints match across wallets exactly when they are
/// signing the same transaction for the same sealer.
export function fingerprint(messageHashHex: string): string {
  return (
    messageHashHex
      .slice(0, 16)
      .toUpperCase()
      .match(/.{1,4}/g)
      ?.join("-") ?? ""
  );
}

/// The value an argument passes to the call. The engine decodes a blob
/// argument exactly like a literal, so both are read here.
function argValue(arg: InstructionArg | undefined, blobs: Blobs): unknown {
  if (arg && typeof arg === "object" && "Literal" in arg) {
    return decode(arg.Literal, { encoding: "hex" });
  }
  if (arg && typeof arg === "object" && "Blob" in arg) {
    const bytes = blobBytes(blobs, arg.Blob);
    if (bytes != null) {
      return decode(bytes);
    }
  }
  throw new Error("argument is not a literal or a blob the transaction carries");
}

function integer(value: unknown): bigint {
  if (typeof value === "number" && Number.isInteger(value)) {
    return BigInt(value);
  }
  if (typeof value === "bigint") {
    return value;
  }
  throw new Error("argument is not an integer");
}

function formatBps(bps: bigint): string {
  const whole = bps / 100n;
  const frac = (bps % 100n).toString().padStart(2, "0");
  return `${whole}.${frac}%`;
}

function callTarget(instruction: Instruction): { address: string; method: string; args: InstructionArg[] } | null {
  if (typeof instruction !== "object" || !("CallMethod" in instruction)) {
    return null;
  }
  const { call, method, args } = instruction.CallMethod;
  if (!("Address" in call)) {
    return null;
  }
  return { address: call.Address, method, args };
}

export interface GovernanceDescription {
  headline: string;
  details: string[];
}

/// Describes a call on the burn-rate governance component, or returns `null`
/// for any other instruction.
export function describeGovernanceCall(instruction: Instruction, blobs: Blobs): GovernanceDescription | null {
  const target = callTarget(instruction);
  if (!target || target.address !== BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS) {
    return null;
  }
  const { method, args } = target;
  try {
    switch (method) {
      case "set_burn_rate": {
        const rate = integer(argValue(args[0], blobs));
        const epoch = integer(argValue(args[1], blobs));
        return {
          headline: `Set the exhaust burn rate to ${formatBps(rate)} (${rate} bps) from epoch ${epoch}`,
          details: [],
        };
      }
      case "set_council": {
        const threshold = integer(argValue(args[0], blobs));
        const council = argValue(args[1], blobs);
        if (!Array.isArray(council)) {
          throw new Error("council is not a list");
        }
        return {
          headline: `Replace the council: ${threshold} of ${council.length} members must sign from now on`,
          details: council.map((member) => toHexString(member)),
        };
      }
      case "retire":
        return {
          headline: "Retire burn-rate governance: the council is dismissed and the release schedule sets the rate",
          details: [],
        };
      default:
        return { headline: `Call "${method}" on burn-rate governance`, details: [] };
    }
  } catch {
    return {
      headline: `Call "${method}" on burn-rate governance (its arguments could not be decoded)`,
      details: [],
    };
  }
}

/// The fee an instruction commits to paying, when it is a `pay_fee` call whose
/// amount is a literal or a blob.
export function describeFee(instruction: Instruction, blobs: Blobs): string | null {
  const target = callTarget(instruction);
  if (!target || target.method !== "pay_fee") {
    return null;
  }
  try {
    const amount = integer(argValue(target.args[0], blobs));
    return `Up to ${amount.toLocaleString()} µT, paid by ${target.address}`;
  } catch {
    return null;
  }
}
