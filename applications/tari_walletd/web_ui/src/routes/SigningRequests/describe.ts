//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

//! Plain-language descriptions of what a signing or transaction request authorises, for the
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

const RESOURCE_ADDRESS_TAG = 131;
const U64_LIMIT = 1n << 64n;

function resourceAddress(value: unknown): string {
  if (typeof value === "object" && value !== null && "tag" in value && "contents" in value) {
    const { tag, contents } = value as { tag: unknown; contents: unknown };
    if (tag === RESOURCE_ADDRESS_TAG && (contents instanceof Uint8Array || Array.isArray(contents))) {
      return `resource_${toHexString(contents)}`;
    }
  }
  throw new Error("argument is not a resource address");
}

/// An `Amount` in any encoding the engine accepts for one: an integer, a
/// decimal string, or its canonical `[low, high]` pair of u64 digits.
function amount(value: unknown): bigint {
  if (typeof value === "string" && /^[0-9]+$/.test(value)) {
    return BigInt(value);
  }
  if (Array.isArray(value) && value.length === 2) {
    const [low, high] = value.map(integer);
    if (low >= 0n && high >= 0n && low < U64_LIMIT && high < U64_LIMIT) {
      return low + (high << 64n);
    }
  }
  const n = integer(value);
  if (n < 0n) {
    throw new Error("amount is negative");
  }
  return n;
}

export type WithdrawnValue =
  | { kind: "amount"; amount: bigint }
  | { kind: "all" }
  | { kind: "non_fungibles"; count: number }
  | { kind: "confidential" };

/// Where a withdrawn bucket goes. `unknown` covers every destination other than
/// a single `deposit` call or fee payment consuming the whole bucket; the
/// instruction list is then the only account of it.
export type WithdrawalDestination =
  | { kind: "deposit"; component: string; isOwnAccount: boolean }
  | { kind: "fee" }
  | { kind: "unknown" };

export interface AccountWithdrawal {
  account: string;
  resource: string;
  value: WithdrawnValue;
  destination: WithdrawalDestination;
}

export interface FeePayment {
  account: string;
  amount: bigint;
}

export interface InstructionsDescription {
  withdrawals: AccountWithdrawal[];
  fees: FeePayment[];
  /// Instructions that are neither part of a described withdrawal or fee
  /// payment nor a stealth transfer, which the wallet summarises itself.
  undescribed: number;
}

function withdrawnValue(method: string, args: InstructionArg[], blobs: Blobs): WithdrawnValue | null {
  switch (method) {
    case "withdraw":
      return { kind: "amount", amount: amount(argValue(args[1], blobs)) };
    case "withdraw_approved":
      return { kind: "amount", amount: amount(argValue(args[2], blobs)) };
    case "withdraw_all":
      return { kind: "all" };
    case "withdraw_non_fungible":
      return { kind: "non_fungibles", count: 1 };
    case "withdraw_many_non_fungibles": {
      const ids = argValue(args[1], blobs);
      if (!Array.isArray(ids)) {
        throw new Error("non-fungible ids are not a list");
      }
      return { kind: "non_fungibles", count: ids.length };
    }
    case "withdraw_confidential":
      return { kind: "confidential" };
    default:
      return null;
  }
}

function workspaceKeyWritten(instruction: Instruction): number | null {
  if (typeof instruction !== "object") {
    return null;
  }
  if ("PutLastInstructionOutputOnWorkspace" in instruction) {
    return instruction.PutLastInstructionOutputOnWorkspace.key;
  }
  if ("TakeFromBucket" in instruction) {
    return instruction.TakeFromBucket.output_bucket;
  }
  if ("AllocateAddress" in instruction) {
    return instruction.AllocateAddress.workspace_id;
  }
  return null;
}

function wholeWorkspaceItem(arg: InstructionArg | undefined): number | null {
  if (arg && typeof arg === "object" && "Workspace" in arg && arg.Workspace.offset == null) {
    return arg.Workspace.id;
  }
  return null;
}

/// Describes the withdrawals from the wallet's own accounts and the fee
/// payments in one instruction list (a transaction's instructions or its fee
/// instructions), following each withdrawn bucket to the `deposit` or fee
/// payment that consumes it. Every other instruction is counted as
/// undescribed, so a caller can tell when the summary is not the whole story.
export function describeInstructions(
  instructions: Instruction[],
  blobs: Blobs,
  ownAccounts: ReadonlySet<string>,
): InstructionsDescription {
  const withdrawals: AccountWithdrawal[] = [];
  const fees: FeePayment[] = [];
  let undescribed = 0;
  // Workspace key -> the withdrawal whose bucket it holds.
  const buckets = new Map<number, AccountWithdrawal>();

  for (let i = 0; i < instructions.length; i++) {
    const instruction = instructions[i];
    const written = workspaceKeyWritten(instruction);
    if (written != null) {
      buckets.delete(written);
    }

    if (typeof instruction === "object" && "StealthTransfer" in instruction) {
      const next = instructions[i + 1];
      if (next && typeof next === "object" && "PutLastInstructionOutputOnWorkspace" in next) {
        buckets.delete(next.PutLastInstructionOutputOnWorkspace.key);
        i++;
      }
      continue;
    }

    if (typeof instruction === "object" && "PayFeeFromBucket" in instruction) {
      const { id, offset } = instruction.PayFeeFromBucket.bucket;
      const withdrawal = offset == null ? buckets.get(id) : undefined;
      if (withdrawal) {
        withdrawal.destination = { kind: "fee" };
        buckets.delete(id);
      }
      continue;
    }

    const target = callTarget(instruction);
    if (target && ownAccounts.has(target.address)) {
      const { address, method, args } = target;
      try {
        if (method === "pay_fee") {
          fees.push({ account: address, amount: amount(argValue(args[0], blobs)) });
          continue;
        }
        const resourceArg = method === "withdraw_approved" ? args[1] : args[0];
        const value = withdrawnValue(method, args, blobs);
        if (value) {
          const withdrawal: AccountWithdrawal = {
            account: address,
            resource: resourceAddress(argValue(resourceArg, blobs)),
            value,
            destination: { kind: "unknown" },
          };
          withdrawals.push(withdrawal);
          const next = instructions[i + 1];
          if (next && typeof next === "object" && "PutLastInstructionOutputOnWorkspace" in next) {
            buckets.set(next.PutLastInstructionOutputOnWorkspace.key, withdrawal);
            i++;
          }
          continue;
        }
      } catch {
        // An argument that cannot be read leaves the call undescribed.
      }
    }

    if (target && target.method === "deposit" && target.args.length === 1) {
      const key = wholeWorkspaceItem(target.args[0]);
      const withdrawal = key != null ? buckets.get(key) : undefined;
      if (key != null && withdrawal) {
        withdrawal.destination = {
          kind: "deposit",
          component: target.address,
          isOwnAccount: ownAccounts.has(target.address),
        };
        buckets.delete(key);
        continue;
      }
    }

    undescribed++;
  }

  return { withdrawals, fees, undescribed };
}

/// The fee an instruction commits to paying, when it is a `pay_fee` call whose
/// amount is a literal or a blob.
export function describeFee(instruction: Instruction, blobs: Blobs): string | null {
  const target = callTarget(instruction);
  if (!target || target.method !== "pay_fee") {
    return null;
  }
  try {
    const fee = amount(argValue(target.args[0], blobs));
    return `Up to ${fee.toLocaleString()} µT, paid by ${target.address}`;
  } catch {
    return null;
  }
}
