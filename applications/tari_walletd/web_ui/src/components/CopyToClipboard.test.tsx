// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { type ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

interface EffectRecord {
  deps?: unknown[];
  cleanup?: (() => void) | void;
  fn: () => (() => void) | void;
  pending: boolean;
}

const hookEnv = {
  states: [] as unknown[],
  refs: [] as Array<{ current: unknown }>,
  effects: [] as EffectRecord[],
  stateCursor: 0,
  refCursor: 0,
  effectCursor: 0,
};

vi.mock("react", async () => {
  const actual = await vi.importActual<typeof import("react")>("react");
  return {
    ...actual,
    useState<T>(initial: T | (() => T)) {
      const idx = hookEnv.stateCursor++;
      if (!(idx in hookEnv.states)) {
        hookEnv.states[idx] = typeof initial === "function" ? (initial as () => T)() : initial;
      }
      const setState = (next: T | ((prev: T) => T)) => {
        hookEnv.states[idx] = typeof next === "function" ? (next as (prev: T) => T)(hookEnv.states[idx] as T) : next;
      };
      return [hookEnv.states[idx] as T, setState] as const;
    },
    useRef<T>(initial: T) {
      const idx = hookEnv.refCursor++;
      if (!(idx in hookEnv.refs)) {
        hookEnv.refs[idx] = { current: initial };
      }
      return hookEnv.refs[idx] as { current: T };
    },
    useEffect(fn: () => (() => void) | void, deps?: unknown[]) {
      const idx = hookEnv.effectCursor++;
      const prev = hookEnv.effects[idx];
      const changed =
        !prev ||
        !deps ||
        !prev.deps ||
        deps.length !== prev.deps.length ||
        deps.some((d, i) => !Object.is(d, prev.deps![i]));
      if (changed) {
        hookEnv.effects[idx] = { deps: deps ? [...deps] : undefined, cleanup: prev?.cleanup, fn, pending: true };
      }
    },
  };
});

import CopyToClipboard from "./CopyToClipboard";

function renderCopy(props: { copy: string; title?: string }) {
  hookEnv.stateCursor = 0;
  hookEnv.refCursor = 0;
  hookEnv.effectCursor = 0;
  const el = CopyToClipboard(props) as ReactElement<any>;
  for (const eff of hookEnv.effects) {
    if (eff.pending) {
      eff.pending = false;
      eff.cleanup?.();
      eff.cleanup = eff.fn();
    }
  }
  return el;
}

function unmountCopy() {
  for (const eff of hookEnv.effects) {
    eff.cleanup?.();
    eff.cleanup = undefined;
  }
}

describe("CopyToClipboard", () => {
  const originalNavigator = globalThis.navigator;

  beforeEach(() => {
    vi.useFakeTimers();
    hookEnv.states = [];
    hookEnv.refs = [];
    hookEnv.effects = [];
  });

  afterEach(() => {
    unmountCopy();
    vi.useRealTimers();
    Object.defineProperty(globalThis, "navigator", { value: originalNavigator, configurable: true, writable: true });
    vi.restoreAllMocks();
  });

  function setClipboard(writeText: ((text: string) => Promise<void>) | undefined) {
    Object.defineProperty(globalThis, "navigator", {
      value: writeText ? { clipboard: { writeText } } : {},
      configurable: true,
      writable: true,
    });
  }

  it("replaces overlapping reset timers on repeated clicks and wraps IconButton inside Tooltip", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    setClipboard(writeText);

    let tree = renderCopy({ copy: "hash_1", title: "Copy hash" });
    expect(tree.props.title).toBe("Copy hash");
    const button = tree.props.children as ReactElement<any>;
    expect(button.props["aria-label"]).toBe("copy to clipboard");

    await button.props.onClick();
    tree = renderCopy({ copy: "hash_1", title: "Copy hash" });
    expect(tree.props.title).toBe("Copied to clipboard");

    // Advance 1500ms and click again: old timer must be cleared so tooltip stays visible at t=2000ms
    vi.advanceTimersByTime(1500);
    await (tree.props.children as ReactElement<any>).props.onClick();

    vi.advanceTimersByTime(600);
    tree = renderCopy({ copy: "hash_1", title: "Copy hash" });
    expect(tree.props.title).toBe("Copied to clipboard");

    // After full 2000ms from second click, tooltip resets
    vi.advanceTimersByTime(1400);
    tree = renderCopy({ copy: "hash_1", title: "Copy hash" });
    expect(tree.props.title).toBe("Copy hash");
  });

  it("resets stale feedback when the copy prop changes and ignores late async resolutions", async () => {
    let resolveFirst!: () => void;
    const writeText = vi.fn().mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveFirst = resolve;
        }),
    );
    setClipboard(writeText);

    let tree = renderCopy({ copy: "addr_1" });
    const pendingCopy = (tree.props.children as ReactElement<any>).props.onClick();

    // Switch copy target before writeText settles
    tree = renderCopy({ copy: "addr_2" });
    expect(tree.props.title).toBe("addr_2");

    resolveFirst();
    await pendingCopy;
    tree = renderCopy({ copy: "addr_2" });
    expect(tree.props.title).toBe("addr_2");
  });

  it("handles missing navigator.clipboard cleanly and clears pending timers on unmount", async () => {
    setClipboard(undefined);

    let tree = renderCopy({ copy: "addr_1" });
    await (tree.props.children as ReactElement<any>).props.onClick();
    tree = renderCopy({ copy: "addr_1" });
    expect(tree.props.title).toBe("Clipboard unavailable");

    expect(vi.getTimerCount()).toBe(1);
    unmountCopy();
    expect(vi.getTimerCount()).toBe(0);
  });
});
