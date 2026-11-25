//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

export const DECRYPT_FAILURE_TEXT = "Failed to decrypt value";

export function parseNonNegativeBigInt(raw: string): bigint | null {
  const trimmed = raw.trim();
  if (!/^\d+$/.test(trimmed)) {
    return null;
  }
  return BigInt(trimmed);
}

export function formatDecryptedBalance(value: number | bigint | null | undefined): string {
  if (value === null || value === undefined) {
    return DECRYPT_FAILURE_TEXT;
  }
  return value.toString();
}

export interface DecryptUtxoFormFields {
  resourceAddress: string;
  utxoId: string;
  keyId: string;
  minimumExpectedValue: string;
  maximumExpectedValue: string;
}

export type DecryptUtxoValidationResult =
  | {
      ok: true;
      resourceAddress: string;
      utxoId: string;
      keyId: bigint;
      minimumExpectedValue: bigint | null;
      maximumExpectedValue: bigint | null;
    }
  | { ok: false; error: string };

export function validateDecryptUtxoForm(fields: DecryptUtxoFormFields): DecryptUtxoValidationResult {
  const resourceAddress = fields.resourceAddress.trim();
  if (!resourceAddress) {
    return { ok: false, error: "Resource Address is required." };
  }

  const utxoId = fields.utxoId.trim();
  if (!utxoId) {
    return { ok: false, error: "UTXO ID is required." };
  }

  const keyId = parseNonNegativeBigInt(fields.keyId);
  if (keyId === null) {
    return { ok: false, error: "Key ID must be a non-negative integer." };
  }

  const minTrimmed = fields.minimumExpectedValue.trim();
  let minimumExpectedValue: bigint | null = null;
  if (minTrimmed !== "") {
    minimumExpectedValue = parseNonNegativeBigInt(minTrimmed);
    if (minimumExpectedValue === null) {
      return { ok: false, error: "Minimum Expected Value must be a non-negative integer." };
    }
  }

  const maxTrimmed = fields.maximumExpectedValue.trim();
  let maximumExpectedValue: bigint | null = null;
  if (maxTrimmed !== "") {
    maximumExpectedValue = parseNonNegativeBigInt(maxTrimmed);
    if (maximumExpectedValue === null) {
      return { ok: false, error: "Maximum Expected Value must be a non-negative integer." };
    }
  }

  if (minimumExpectedValue !== null && maximumExpectedValue !== null && minimumExpectedValue > maximumExpectedValue) {
    return { ok: false, error: "Minimum Expected Value cannot exceed Maximum Expected Value." };
  }

  return {
    ok: true,
    resourceAddress,
    utxoId,
    keyId,
    minimumExpectedValue,
    maximumExpectedValue,
  };
}

export type ViewVaultValidationResult = { ok: true; vaultId: string; viewKeyId: bigint } | { ok: false; error: string };

export function validateViewVaultForm(fields: { vaultId: string; keyId: string }): ViewVaultValidationResult {
  const vaultId = fields.vaultId.trim();
  if (!vaultId) {
    return { ok: false, error: "Vault ID is required." };
  }

  const viewKeyId = parseNonNegativeBigInt(fields.keyId);
  if (viewKeyId === null) {
    return { ok: false, error: "Key ID must be a non-negative integer." };
  }

  return { ok: true, vaultId, viewKeyId };
}
