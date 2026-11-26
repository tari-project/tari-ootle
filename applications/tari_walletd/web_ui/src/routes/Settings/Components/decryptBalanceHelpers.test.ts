//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import {
  DECRYPT_FAILURE_TEXT,
  formatDecryptedBalance,
  parseNonNegativeBigInt,
  validateDecryptUtxoForm,
  validateViewVaultForm,
} from "./decryptBalanceHelpers";

describe("formatDecryptedBalance", () => {
  it("formats zero values as '0' instead of reporting a decryption failure", () => {
    expect(formatDecryptedBalance(0)).toBe("0");
    expect(formatDecryptedBalance(0n)).toBe("0");
  });

  it("formats positive numbers and bigints as strings", () => {
    expect(formatDecryptedBalance(42)).toBe("42");
    expect(formatDecryptedBalance(1_000_000n)).toBe("1000000");
  });

  it("formats null and undefined as the failure message", () => {
    expect(formatDecryptedBalance(null)).toBe(DECRYPT_FAILURE_TEXT);
    expect(formatDecryptedBalance(undefined)).toBe(DECRYPT_FAILURE_TEXT);
  });
});

describe("parseNonNegativeBigInt", () => {
  it("parses zero and positive integers", () => {
    expect(parseNonNegativeBigInt("0")).toBe(0n);
    expect(parseNonNegativeBigInt(" 100000000 ")).toBe(100000000n);
  });

  it("returns null for empty, negative, fractional, scientific-notation, and non-numeric strings", () => {
    expect(parseNonNegativeBigInt("")).toBeNull();
    expect(parseNonNegativeBigInt("   ")).toBeNull();
    expect(parseNonNegativeBigInt("-1")).toBeNull();
    expect(parseNonNegativeBigInt("0.5")).toBeNull();
    expect(parseNonNegativeBigInt("1e6")).toBeNull();
    expect(parseNonNegativeBigInt("abc")).toBeNull();
  });
});

describe("validateDecryptUtxoForm", () => {
  const validFields = {
    resourceAddress: "resource_0000",
    utxoId: "0001",
    keyId: "0",
    minimumExpectedValue: "",
    maximumExpectedValue: "100000000",
  };

  it("accepts valid form inputs including keyId 0 and optional min/max", () => {
    expect(validateDecryptUtxoForm(validFields)).toEqual({
      ok: true,
      resourceAddress: "resource_0000",
      utxoId: "0001",
      keyId: 0n,
      minimumExpectedValue: null,
      maximumExpectedValue: 100000000n,
    });
    expect(validateDecryptUtxoForm({ ...validFields, minimumExpectedValue: "0", maximumExpectedValue: "" })).toEqual({
      ok: true,
      resourceAddress: "resource_0000",
      utxoId: "0001",
      keyId: 0n,
      minimumExpectedValue: 0n,
      maximumExpectedValue: null,
    });
  });

  it("rejects missing required fields, non-integer inputs, and inverted min/max ranges", () => {
    expect(validateDecryptUtxoForm({ ...validFields, resourceAddress: "  " }).ok).toBe(false);
    expect(validateDecryptUtxoForm({ ...validFields, utxoId: "" }).ok).toBe(false);
    expect(validateDecryptUtxoForm({ ...validFields, keyId: "0.5" }).ok).toBe(false);
    expect(validateDecryptUtxoForm({ ...validFields, minimumExpectedValue: "-5" }).ok).toBe(false);
    expect(validateDecryptUtxoForm({ ...validFields, maximumExpectedValue: "abc" }).ok).toBe(false);
    expect(
      validateDecryptUtxoForm({ ...validFields, minimumExpectedValue: "500", maximumExpectedValue: "100" }),
    ).toEqual({
      ok: false,
      error: "Minimum Expected Value cannot exceed Maximum Expected Value.",
    });
  });
});

describe("validateViewVaultForm", () => {
  it("accepts valid vaultId and keyId including 0", () => {
    expect(validateViewVaultForm({ vaultId: "vault_00", keyId: "0" })).toEqual({
      ok: true,
      vaultId: "vault_00",
      viewKeyId: 0n,
    });
  });

  it("rejects blank vaultId or non-integer keyId", () => {
    expect(validateViewVaultForm({ vaultId: "", keyId: "0" }).ok).toBe(false);
    expect(validateViewVaultForm({ vaultId: "vault_00", keyId: "0.5" }).ok).toBe(false);
    expect(validateViewVaultForm({ vaultId: "vault_00", keyId: "-1" }).ok).toBe(false);
  });
});
