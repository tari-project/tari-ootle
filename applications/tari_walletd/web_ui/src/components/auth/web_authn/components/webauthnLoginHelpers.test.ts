// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { Buffer } from "buffer";
import { describe, expect, it } from "vitest";
import {
  assertWebauthnGetSupported,
  formatWebauthnLoginError,
  parseWebauthnRequestOptions,
} from "./webauthnLoginHelpers";

describe("webauthnLoginHelpers", () => {
  it("parses challenge and preserves transports when allowCredentials is provided", () => {
    const result = parseWebauthnRequestOptions({
      publicKey: {
        challenge: "AAECAw",
        allowCredentials: [{ id: "BAUGBw", type: "public-key", transports: ["usb", "internal"] }],
      },
    });

    expect(result.challenge.equals(Buffer.from("AAECAw", "base64"))).toBe(true);
    expect(result.allowCredentials).toHaveLength(1);
    expect(Buffer.from(result.allowCredentials[0].id as Uint8Array).equals(Buffer.from("BAUGBw", "base64"))).toBe(true);
    expect(result.allowCredentials[0].type).toBe("public-key");
    expect(result.allowCredentials[0].transports).toEqual(["usb", "internal"]);
  });

  it("handles omitted allowCredentials without throwing", () => {
    const result = parseWebauthnRequestOptions({
      publicKey: { challenge: "AAECAw" },
    });

    expect(result.allowCredentials).toEqual([]);
  });

  it("throws when publicKey or challenge is missing", () => {
    expect(() => parseWebauthnRequestOptions(null)).toThrow("Failed to get challenge");
    expect(() => parseWebauthnRequestOptions({ publicKey: {} })).toThrow("Failed to get challenge");
  });

  it("throws a clear error when navigator.credentials.get is unavailable", () => {
    expect(() => assertWebauthnGetSupported(undefined)).toThrow("HTTPS or localhost");
  });

  it("formats RPC error objects and circular objects safely", () => {
    expect(formatWebauthnLoginError({ code: -32603, message: "Passkey not recognized" })).toBe(
      "Passkey not recognized",
    );
    const circular: Record<string, unknown> = {};
    circular.self = circular;
    expect(formatWebauthnLoginError(circular)).toBe("An unknown error occurred.");
  });
});
