// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resolveActiveTabIndex } from "./settingsTabsHelpers";

const hookState: { states: unknown[]; cursor: number } = { states: [], cursor: 0 };

vi.mock("@mui/material/styles", () => ({
  useTheme: () => ({ spacing: (n: number) => `${n * 8}px` }),
}));

vi.mock("react", async () => {
  const actual = await vi.importActual<typeof import("react")>("react");
  const useState = <T,>(initial: T | (() => T)) => {
    const idx = hookState.cursor++;
    if (!(idx in hookState.states)) {
      hookState.states[idx] = typeof initial === "function" ? (initial as () => T)() : initial;
    }
    const setState = (next: T | ((prev: T) => T)) => {
      hookState.states[idx] = typeof next === "function" ? (next as (prev: T) => T)(hookState.states[idx] as T) : next;
    };
    return [hookState.states[idx] as T, setState] as const;
  };
  return {
    ...actual,
    default: { ...actual, useState },
    useState,
  };
});

import type { ISettingsMenu } from "../Settings";
import SettingsTabs from "./SettingsTabs";

function makeMenu(labels: string[]): ISettingsMenu[] {
  return labels.map((label) => ({
    label,
    title: `${label} Title`,
    content: label,
  }));
}

function findElements(node: ReactNode, predicate: (el: ReactElement<any>) => boolean, acc: ReactElement<any>[] = []) {
  if (Array.isArray(node)) {
    for (const child of node) findElements(child, predicate, acc);
    return acc;
  }
  if (!isValidElement(node)) {
    return acc;
  }
  const el = node as ReactElement<any>;
  if (predicate(el)) {
    acc.push(el);
  }
  if (el.props?.children !== undefined) {
    findElements(el.props.children, predicate, acc);
  }
  return acc;
}

describe("resolveActiveTabIndex", () => {
  it("returns false when menuItems is empty", () => {
    expect(resolveActiveTabIndex([], null)).toBe(false);
    expect(resolveActiveTabIndex([], "General")).toBe(false);
  });

  it("tracks the selected label across dynamic tab insertions and removals", () => {
    const before = makeMenu(["General", "Accounts", "Access Tokens", "View Vault Balance", "Advanced"]);
    const withPasskeys = makeMenu([
      "General",
      "Accounts",
      "Access Tokens",
      "Passkeys",
      "View Vault Balance",
      "Advanced",
    ]);

    expect(resolveActiveTabIndex(before, "Advanced")).toBe(4);
    expect(resolveActiveTabIndex(withPasskeys, "Advanced")).toBe(5);
    expect(resolveActiveTabIndex(before, "Passkeys")).toBe(0);
    expect(resolveActiveTabIndex(before, null)).toBe(0);
  });
});

describe("SettingsTabs", () => {
  beforeEach(() => {
    hookState.states = [];
    hookState.cursor = 0;
  });

  const renderTabs = (menuItems: ISettingsMenu[]) => {
    hookState.cursor = 0;
    return SettingsTabs({ menuItems });
  };

  it("keeps the user on the same logical tab when Passkeys is inserted dynamically and falls back when removed", () => {
    const baseItems = makeMenu(["General", "Accounts", "Access Tokens", "View Vault Balance", "Advanced"]);
    const webauthnItems = makeMenu([
      "General",
      "Accounts",
      "Access Tokens",
      "Passkeys",
      "View Vault Balance",
      "Advanced",
    ]);

    let tree = renderTabs(baseItems);
    let tabs = findElements(tree, (el) => el.props?.["aria-label"] === "settings tabs")[0];
    expect(tabs.props.value).toBe(0);

    // Select "Advanced" (index 4)
    tabs.props.onChange({} as React.SyntheticEvent, 4);
    tree = renderTabs(baseItems);
    tabs = findElements(tree, (el) => el.props?.["aria-label"] === "settings tabs")[0];
    expect(tabs.props.value).toBe(4);

    // Insert "Passkeys" at index 3: "Advanced" shifts to index 5 and stays active
    tree = renderTabs(webauthnItems);
    tabs = findElements(tree, (el) => el.props?.["aria-label"] === "settings tabs")[0];
    expect(tabs.props.value).toBe(5);

    // Select "Passkeys" (index 3), then remove it: falls back cleanly to index 0
    tabs.props.onChange({} as React.SyntheticEvent, 3);
    tree = renderTabs(webauthnItems);
    tabs = findElements(tree, (el) => el.props?.["aria-label"] === "settings tabs")[0];
    expect(tabs.props.value).toBe(3);

    tree = renderTabs(baseItems);
    tabs = findElements(tree, (el) => el.props?.["aria-label"] === "settings tabs")[0];
    expect(tabs.props.value).toBe(0);

    // Keys on Tab elements match item labels rather than unstable array indices
    const tabElements = findElements(tree, (el) => typeof el.props?.label === "string");
    expect(tabElements.map((el) => el.key)).toEqual(baseItems.map((i) => i.label));
  });
});
