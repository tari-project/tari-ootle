// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

export function assertWebauthnCreateSupported(credentials?: CredentialsContainer): CredentialsContainer {
  if (!credentials || typeof credentials.create !== "function") {
    throw new Error("WebAuthn is not available in this browser context. Open the wallet over HTTPS or localhost.");
  }
  return credentials;
}

export function resolveBackupPasskeyLabel(label: string): string {
  const trimmed = label.trim();
  return trimmed || "Backup Passkey";
}

export function formatWebauthnRegistrationError(error: unknown): string {
  if (error instanceof Error) {
    return error.message || "Passkey registration failed.";
  }
  if (typeof error === "string") {
    return error || "Passkey registration failed.";
  }
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) {
      return message.trim();
    }
  }
  return "Passkey registration failed.";
}
