// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { Buffer } from "buffer";

interface RawAllowCredential {
  id?: unknown;
  type?: unknown;
  transports?: unknown;
}

interface RawChallengeResponse {
  publicKey?: {
    challenge?: unknown;
    allowCredentials?: unknown;
  };
}

export function parseWebauthnRequestOptions(challengeResponse: unknown): {
  challenge: Buffer;
  allowCredentials: PublicKeyCredentialDescriptor[];
} {
  const publicKey = (challengeResponse as RawChallengeResponse | null | undefined)?.publicKey;
  if (!publicKey || typeof publicKey.challenge !== "string" || publicKey.challenge.length === 0) {
    throw new Error("Failed to get challenge");
  }

  const challenge = Buffer.from(publicKey.challenge, "base64");
  const rawCredentials = Array.isArray(publicKey.allowCredentials) ? publicKey.allowCredentials : [];
  const allowCredentials: PublicKeyCredentialDescriptor[] = [];

  for (const raw of rawCredentials) {
    const cred = raw as RawAllowCredential | null | undefined;
    if (!cred || typeof cred.id !== "string" || cred.id.length === 0) {
      continue;
    }
    const descriptor: PublicKeyCredentialDescriptor = {
      id: Buffer.from(cred.id, "base64"),
      type: typeof cred.type === "string" ? (cred.type as PublicKeyCredentialType) : "public-key",
    };
    if (Array.isArray(cred.transports)) {
      descriptor.transports = cred.transports as AuthenticatorTransport[];
    }
    allowCredentials.push(descriptor);
  }

  return { challenge, allowCredentials };
}

export function assertWebauthnGetSupported(credentials?: CredentialsContainer): CredentialsContainer {
  if (!credentials || typeof credentials.get !== "function") {
    throw new Error("WebAuthn is not available in this browser context. Open the wallet over HTTPS or localhost.");
  }
  return credentials;
}

export function formatWebauthnLoginError(error: unknown): string {
  if (error instanceof Error) {
    return error.message || "Authentication failed.";
  }
  if (typeof error === "string") {
    return error || "Authentication failed.";
  }
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) {
      return message.trim();
    }
  }
  try {
    return "An unknown error occurred: " + JSON.stringify(error);
  } catch {
    return "An unknown error occurred.";
  }
}
