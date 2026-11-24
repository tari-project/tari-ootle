//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import React from "react";
import { describe, expect, it } from "vitest";
import { extractBreadcrumbText, resolveBreadcrumbItem } from "./breadcrumbsHelpers";

describe("breadcrumbsHelpers", () => {
  it("extracts text safely from elements, primitives, arrays, and nullish nodes", () => {
    expect(extractBreadcrumbText(React.createElement("span", null, "Account Details"))).toBe("Account Details");
    expect(extractBreadcrumbText("Transactions")).toBe("Transactions");
    expect(extractBreadcrumbText(42)).toBe("42");
    expect(extractBreadcrumbText([React.createElement("span", null, "Tx "), 7])).toBe("Tx 7");
    expect(extractBreadcrumbText(null)).toBe("");
    expect(extractBreadcrumbText(undefined)).toBe("");
  });

  it("uses resolved match.pathname instead of literal dynamic route patterns", () => {
    const item = resolveBreadcrumbItem(
      {
        pathname: "/accounts/alice",
        route: { label: "Account Details", path: "/accounts/:id", dynamic: true },
      },
      React.createElement("span", null, "Alice"),
      1,
      3,
    );

    expect(item).toEqual({
      key: "/accounts/alice-1",
      label: "alice",
      to: "/accounts/alice",
      isLast: false,
    });
  });

  it("uses static route labels, terminal lowercase text, and unique keys", () => {
    const first = resolveBreadcrumbItem(
      {
        pathname: "/accounts",
        route: { label: "Accounts", path: "/accounts", dynamic: false },
      },
      React.createElement("span", null, "Accounts"),
      0,
      2,
    );
    const last = resolveBreadcrumbItem(
      {
        pathname: "/accounts/accounts",
        route: { label: "Account Details", path: "/accounts/:id", dynamic: true },
      },
      React.createElement("span", null, "Accounts"),
      1,
      2,
    );

    expect(first).toEqual({
      key: "/accounts-0",
      label: "Accounts",
      to: "/accounts",
      isLast: false,
    });
    expect(last).toEqual({
      key: "/accounts/accounts-1",
      label: "accounts",
      to: null,
      isLast: true,
    });
    expect(first.key).not.toBe(last.key);
  });

  it("falls back to root path for unmatched intermediate routes", () => {
    const fallback = resolveBreadcrumbItem(
      { pathname: "/unknown" },
      React.createElement("span", null, "Unknown"),
      0,
      2,
    );
    expect(fallback.to).toBe("/");
    expect(fallback.label).toBe("unknown");
  });
});
