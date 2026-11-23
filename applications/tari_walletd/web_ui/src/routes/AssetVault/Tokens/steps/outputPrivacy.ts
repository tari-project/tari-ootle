//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { ResourceType } from "@tari-project/ootle-ts-bindings";

export interface OutputPrivacyDisplay {
  label: string;
  value: string;
  isRevealed: boolean;
}

export function getOutputPrivacyDisplay(
  resourceType: ResourceType,
  outputToRevealed: boolean,
): OutputPrivacyDisplay | null {
  if (resourceType === "Confidential") {
    return {
      label: "Send Confidential Outputs:",
      value: outputToRevealed ? "No (Revealed)" : "Yes",
      isRevealed: outputToRevealed,
    };
  }
  if (resourceType === "Stealth") {
    return {
      label: "Send Stealth Outputs:",
      value: outputToRevealed ? "No (Revealed)" : "Yes",
      isRevealed: outputToRevealed,
    };
  }
  return null;
}
