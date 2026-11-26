// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import {
  assertWebauthnCreateSupported,
  formatWebauthnRegistrationError,
  resolveBackupPasskeyLabel,
} from "./webauthnRegistrationHelpers";

describe("webauthnRegistrationHelpers", () => {
  it("throws a clear error when navigator.credentials.create is unavailable", () => {
    expect(() => assertWebauthnCreateSupported(undefined)).toThrow("HTTPS or localhost");
  });

  it("trims backup passkey labels and falls back on whitespace-only input", () => {
    expect(resolveBackupPasskeyLabel("  YubiKey 5  ")).toBe("YubiKey 5");
    expect(resolveBackupPasskeyLabel("   ")).toBe("Backup Passkey");
    expect(resolveBackupPasskeyLabel("")).toBe("Backup Passkey");
  });

  it("formats RPC error objects, Errors, strings, and unknown values without [object Object]", () => {
    expect(formatWebauthnRegistrationError({ code: -32602, message: "Invalid setup token" })).toBe(
      "Invalid setup token",
    );
    expect(formatWebauthnRegistrationError(new Error("User cancelled"))).toBe("User cancelled");
    expect(formatWebauthnRegistrationError("Registration rejected")).toBe("Registration rejected");
    expect(formatWebauthnRegistrationError({ code: -32000 })).toBe("Passkey registration failed.");
    expect(formatWebauthnRegistrationError(null)).toBe("Passkey registration failed.");
  });
});
