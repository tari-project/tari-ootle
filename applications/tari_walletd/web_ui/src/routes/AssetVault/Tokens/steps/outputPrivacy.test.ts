//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { getOutputPrivacyDisplay } from "./outputPrivacy";

describe("getOutputPrivacyDisplay", () => {
  it("reports confidential outputs as Yes when outputToRevealed is false", () => {
    expect(getOutputPrivacyDisplay("Confidential", false)).toEqual({
      label: "Send Confidential Outputs:",
      value: "Yes",
      isRevealed: false,
    });
  });

  it("reports confidential outputs as No (Revealed) when outputToRevealed is true", () => {
    expect(getOutputPrivacyDisplay("Confidential", true)).toEqual({
      label: "Send Confidential Outputs:",
      value: "No (Revealed)",
      isRevealed: true,
    });
  });

  it("reports stealth outputs as Yes when outputToRevealed is false", () => {
    expect(getOutputPrivacyDisplay("Stealth", false)).toEqual({
      label: "Send Stealth Outputs:",
      value: "Yes",
      isRevealed: false,
    });
  });

  it("reports stealth outputs as No (Revealed) when outputToRevealed is true", () => {
    expect(getOutputPrivacyDisplay("Stealth", true)).toEqual({
      label: "Send Stealth Outputs:",
      value: "No (Revealed)",
      isRevealed: true,
    });
  });

  it("returns null for public Fungible and NonFungible resources", () => {
    expect(getOutputPrivacyDisplay("Fungible", false)).toBeNull();
    expect(getOutputPrivacyDisplay("Fungible", true)).toBeNull();
    expect(getOutputPrivacyDisplay("NonFungible", false)).toBeNull();
    expect(getOutputPrivacyDisplay("NonFungible", true)).toBeNull();
  });
});
