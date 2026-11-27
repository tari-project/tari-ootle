//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import { isExactMatchNavRoute } from "./menuItemsHelpers";

describe("isExactMatchNavRoute", () => {
  it("requires exact matching for the root Home route", () => {
    expect(isExactMatchNavRoute("/")).toBe(true);
  });

  it("allows prefix matching for section routes so nested detail pages stay active", () => {
    expect(isExactMatchNavRoute("transactions")).toBe(false);
    expect(isExactMatchNavRoute("templates")).toBe(false);
    expect(isExactMatchNavRoute("settings")).toBe(false);
  });
});
