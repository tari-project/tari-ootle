//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { Permission } from "../types/Permission";
import { RejectReason } from "../types/RejectReason";
import { SubstateDiff } from "../types/SubstateDiff";
import { SubstateId } from "../types/SubstateId";
import { TransactionResult } from "../types/TransactionResult";
import { NonFungibleId } from "../types/NonFungibleId";

// TODO: this function should be deprecated
export function substateIdToString(substateId: SubstateId | string | null | undefined): string {
  if (substateId === null || substateId === undefined) {
    return "";
  }
  if (typeof substateId === "string") {
    return substateId;
  }
  if (typeof substateId !== "object") {
    throw new Error(`Cannot convert: ${JSON.stringify(substateId)} to string`);
  }
  // if ("Component" in substateId) {
  //   return substateId.Component;
  // }
  // if ("Resource" in substateId) {
  //   return substateId.Resource;
  // }
  // if ("Vault" in substateId) {
  //   return substateId.Vault;
  // }
  // if ("ClaimedOutputTombstone" in substateId) {
  //   return substateId.ClaimedOutputTombstone;
  // }
  // if ("NonFungible" in substateId) {
  //   const nft = substateId.NonFungible;
  //   const key = Object.keys(nft.id)[0];
  //   const type = key.toLowerCase();
  //   return `nft_${nft.resource_address}_${type}_${nft.id[key]}`;
  // }
  // if ("TransactionReceipt" in substateId) {
  //   return substateId.TransactionReceipt;
  // }
  // if ("ValidatorFeePool" in substateId) {
  //   return substateId.ValidatorFeePool;
  // }
  console.error("Unknown substate id", substateId);
  return substateId;
}

// TODO: this function should be deprecated
export function stringToSubstateId(substateId: string): SubstateId {
  const parts = splitOnce(substateId, "_");
  if (!parts) {
    throw new Error(`Invalid substate id: ${substateId}`);
  }
  return substateId;
}

export function shortenSubstateId(
  substateId: SubstateId | string | null | undefined,
  start: number = 4,
  end: number = 4,
) {
  if (substateId === null || substateId === undefined) {
    return "";
  }
  const string = substateIdToString(substateId);
  const parts = string.split("_", 2);
  if (parts.length < 2) {
    return string;
  }
  return parts[0] + "_" + shortenString(parts[1], start, end);
}

export function shortenString(string: string, start: number = 8, end: number = 8) {
  return string.substring(0, start) + "..." + string.slice(-end);
}

export function rejectReasonToString(reason: RejectReason | null): string {
  if (reason === null) {
    return "";
  }
  if (typeof reason === "string") {
    return reason;
  }
  if ("ShardsNotPledged" in reason) {
    return `ShardsNotPledged: ${reason.ShardsNotPledged}`;
  }
  if ("ExecutionFailure" in reason) {
    return `ExecutionFailure: ${reason.ExecutionFailure}`;
  }
  if ("ShardPledgedToAnotherPayload" in reason) {
    return `ShardPledgedToAnotherPayload: ${reason.ShardPledgedToAnotherPayload}`;
  }
  if ("ShardRejected" in reason) {
    return `ShardRejected: ${reason.ShardRejected}`;
  }
  if ("InsufficientFeesPaid" in reason) {
    return `InsufficientFeesPaid: ${reason.InsufficientFeesPaid}`;
  }
  if ("ForeignShardGroupDecidedToAbort" in reason) {
    const r = reason.ForeignShardGroupDecidedToAbort;
    return `ForeignShardGroupDecidedToAbort: ${r.start_shard}-${r.end_shard}, ${r.abort_reason}`;
  }
  if ("InvalidTransaction" in reason) {
    return `InvalidTransaction: ${reason.InvalidTransaction}`;
  }
  if ("ExecutionFailure" in reason) {
    return `ExecutionFailure: ${reason.ExecutionFailure}`;
  }
  if ("OneOrMoreInputsNotFound" in reason) {
    return `OneOrMoreInputsNotFound: ${reason.OneOrMoreInputsNotFound}`;
  }
  if ("FailedToLockInputs" in reason) {
    return `FailedToLockInputs: ${reason.FailedToLockInputs}`;
  }
  if ("FailedToLockOutputs" in reason) {
    return `FailedToLockOutputs: ${reason.FailedToLockOutputs}`;
  }
  console.error("Unknown reason", reason);
  return JSON.stringify(reason);
}

export function getSubstateDiffFromTransactionResult(result: TransactionResult): SubstateDiff | null {
  if ("Accept" in result) {
    return result.Accept;
  }
  if ("AcceptFeeRejectRest" in result) {
    return result.AcceptFeeRejectRest[0];
  }
  return null;
}

export function getRejectReasonFromTransactionResult(result: TransactionResult): RejectReason | null {
  if ("Reject" in result) {
    return result.Reject;
  }
  if ("AcceptFeeRejectRest" in result) {
    return result.AcceptFeeRejectRest[1];
  }
  return null;
}

// Render a Permission to its canonical grant string, e.g.
//   `admin`, `webrtc`, `accounts:read`, `transfer:create:component_abc…`,
//   `substates:read`. Round-trips with the Rust `Permission::FromStr`.
export function permissionToString(permission: Permission): string {
  if (typeof permission === "string") {
    // Bare variants (`Admin`, `Webrtc`) serialise to lowercase strings.
    return permission.toLowerCase();
  }

  // Read-only resources: { Substates: "Read" } / { BurnProofs: "Read" } /
  // { SwapPools: "Read" } — render with the resource name in snake_case
  // plus explicit `:read`.
  if ("Substates" in permission) return "substates:read";
  if ("BurnProofs" in permission) return "burn_proofs:read";
  if ("SwapPools" in permission) return "swap_pools:read";

  // Unscoped CRUD resources: { Keys: "Read" } -> "keys:read".
  if ("Keys" in permission) return `keys:${permission.Keys.toLowerCase()}`;
  if ("Templates" in permission) return `templates:${permission.Templates.toLowerCase()}`;
  if ("Transactions" in permission) return `transactions:${permission.Transactions.toLowerCase()}`;
  if ("Validators" in permission) return `validators:${permission.Validators.toLowerCase()}`;
  if ("Settings" in permission) return `settings:${permission.Settings.toLowerCase()}`;
  if ("AddressBook" in permission) return `address_book:${permission.AddressBook.toLowerCase()}`;

  // Scoped CRUD resources: { Accounts: ["Read", "component_abc..." | null] }.
  if ("Accounts" in permission) return scoped("accounts", permission.Accounts);
  if ("Transfer" in permission) return scoped("transfer", permission.Transfer);
  if ("Nfts" in permission) return scoped("nfts", permission.Nfts);
  if ("Confidential" in permission) return scoped("confidential", permission.Confidential);
  if ("StealthUtxos" in permission) return scoped("stealth_utxos", permission.StealthUtxos);

  console.error("Unknown permission", permission);
  return JSON.stringify(permission);
}

function scoped(resource: string, value: [string, string | null]): string {
  const [action, entity] = value;
  const base = `${resource}:${action.toLowerCase()}`;
  return entity ? `${base}:${entity}` : base;
}

function splitOnce(str: string, separator: string): [string, string] | null {
  const index = str.indexOf(separator);
  if (index === -1) {
    return null;
  }
  return [str.slice(0, index), str.slice(index + 1)];
}
