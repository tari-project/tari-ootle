//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export type SubstateDisplayType =
  | "Component"
  | "Vault"
  | "Resource"
  | "NFT"
  | "Claimed Output Tombstone"
  | "Transaction Receipt"
  | "Template"
  | "Utxo"
  | "VnFeePool"
  | "Confidential Output"
  | "Unknown";

export type SubstateChipColor = "primary" | "secondary" | "success" | "warning" | "info" | "error";

export function getSubstateType(substateId: string): SubstateDisplayType {
  if (substateId.startsWith("component_")) return "Component";
  if (substateId.startsWith("vault_")) return "Vault";
  if (substateId.startsWith("resource_")) return "Resource";
  if (substateId.startsWith("nft_")) return "NFT";
  if (substateId.startsWith("tombstone_")) return "Claimed Output Tombstone";
  if (substateId.startsWith("txreceipt_")) return "Transaction Receipt";
  if (substateId.startsWith("template_")) return "Template";
  if (substateId.startsWith("utxo_")) return "Utxo";
  if (substateId.startsWith("vnfp_")) return "VnFeePool";
  if (substateId.startsWith("coutput_")) return "Confidential Output";
  return "Unknown";
}

export function getSubstateTypeColor(type: SubstateDisplayType): SubstateChipColor {
  switch (type) {
    case "Component":
      return "primary";
    case "Vault":
      return "success";
    case "Resource":
      return "secondary";
    case "NFT":
      return "info";
    case "Claimed Output Tombstone":
      return "warning";
    case "Confidential Output":
      return "warning";
    case "Transaction Receipt":
      return "error";
    case "Template":
      return "info";
    case "Utxo":
      return "success";
    case "VnFeePool":
      return "secondary";
    case "Unknown":
      return "primary";
  }
}
