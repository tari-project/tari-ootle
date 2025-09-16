//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { DecodedOotleAddress, encodeOotleAddress, decodeOotleAddress } from "../src/helpers/ootleAddress";

describe("OotleAddress de/encoding", () => {
  it("encodes and decodes a valid address", () => {
    const sample = {
      network: "mainnet",
      accountPublicKey: "a1b2c3d4e5f60718293a4b5c6d7e8f90123456789abcdef0123456789abcdef0",
      viewOnlyKey: "0fedcba9876543210fedcba9876543210fedcba9876543210fedcba98765432a",
    } as DecodedOotleAddress;

    const address = encodeOotleAddress(sample);

    expect(() => {
      let parsed = decodeOotleAddress(address);
      expect(parsed).toEqual(sample);
    }).not.toThrow();
  });

  it("throws on invalid address", () => {
    expect(() => decodeOotleAddress("invalidAddress")).toThrow();
  });
});
