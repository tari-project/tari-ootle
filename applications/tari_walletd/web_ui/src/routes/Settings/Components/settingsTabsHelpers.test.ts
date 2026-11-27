// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { resolveActiveTabIndex } from "./settingsTabsHelpers";

function makeItems(labels: string[]) {
  return labels.map((label) => ({ label }));
}

describe("resolveActiveTabIndex", () => {
  it("returns false when menuItems is empty", () => {
    expect(resolveActiveTabIndex([], null)).toBe(false);
    expect(resolveActiveTabIndex([], "General")).toBe(false);
  });

  it("defaults to index 0 when no label is selected or the selected label is absent", () => {
    const items = makeItems(["General", "Accounts", "Advanced"]);
    expect(resolveActiveTabIndex(items, null)).toBe(0);
    expect(resolveActiveTabIndex(items, "RemovedTab")).toBe(0);
  });

  it("tracks the selected label when items are inserted or reordered", () => {
    const before = makeItems(["General", "Accounts", "Access Tokens", "View Vault Balance", "Advanced"]);
    const afterInsert = makeItems([
      "General",
      "Accounts",
      "Access Tokens",
      "Passkeys",
      "View Vault Balance",
      "Advanced",
    ]);

    expect(resolveActiveTabIndex(before, "Advanced")).toBe(4);
    expect(resolveActiveTabIndex(afterInsert, "Advanced")).toBe(5);
    expect(resolveActiveTabIndex(afterInsert, "Passkeys")).toBe(3);
  });
});
