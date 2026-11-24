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

interface MemoRecord {
  deps?: unknown[];
  value: unknown;
}

const hookEnv = {
  states: [] as unknown[],
  refs: [] as Array<{ current: unknown }>,
  effects: [] as EffectRecord[],
  memos: [] as MemoRecord[],
  stateCursor: 0,
  refCursor: 0,
  effectCursor: 0,
  memoCursor: 0,
};

function depsChanged(prev?: unknown[], next?: unknown[]) {
  if (!prev || !next || prev.length !== next.length) return true;
  return next.some((d, i) => !Object.is(d, prev[i]));
}

vi.mock("react", async () => {
  const actual = await vi.importActual<typeof import("react")>("react");
  const useState = <T,>(initial: T | (() => T)) => {
    const idx = hookEnv.stateCursor++;
    if (!(idx in hookEnv.states)) {
      hookEnv.states[idx] = typeof initial === "function" ? (initial as () => T)() : initial;
    }
    const setState = (next: T | ((prev: T) => T)) => {
      hookEnv.states[idx] = typeof next === "function" ? (next as (prev: T) => T)(hookEnv.states[idx] as T) : next;
    };
    return [hookEnv.states[idx] as T, setState] as const;
  };
  const useRef = <T,>(initial: T) => {
    const idx = hookEnv.refCursor++;
    if (!(idx in hookEnv.refs)) {
      hookEnv.refs[idx] = { current: initial };
    }
    return hookEnv.refs[idx] as { current: T };
  };
  const useEffect = (fn: () => (() => void) | void, deps?: unknown[]) => {
    const idx = hookEnv.effectCursor++;
    const prev = hookEnv.effects[idx];
    if (!prev || depsChanged(prev.deps, deps)) {
      hookEnv.effects[idx] = { deps: deps ? [...deps] : undefined, cleanup: prev?.cleanup, fn, pending: true };
    }
  };
  const useMemo = <T,>(factory: () => T, deps?: unknown[]): T => {
    const idx = hookEnv.memoCursor++;
    const prev = hookEnv.memos[idx];
    if (!prev || depsChanged(prev.deps, deps)) {
      hookEnv.memos[idx] = { deps: deps ? [...deps] : undefined, value: factory() };
    }
    return hookEnv.memos[idx].value as T;
  };
  const useCallback = <T extends (...args: any[]) => any>(fn: T, deps: unknown[]): T => useMemo(() => fn, deps);

  return {
    ...actual,
    default: { ...actual, useState, useRef, useEffect, useMemo, useCallback },
    useState,
    useRef,
    useEffect,
    useMemo,
    useCallback,
  };
});

import { ErrorNotificationProvider } from "./ErrorNotificationContext";

function renderProvider() {
  hookEnv.stateCursor = 0;
  hookEnv.refCursor = 0;
  hookEnv.effectCursor = 0;
  hookEnv.memoCursor = 0;
  const el = ErrorNotificationProvider({ children: null }) as ReactElement<any>;
  for (const eff of hookEnv.effects) {
    if (eff.pending) {
      eff.pending = false;
      eff.cleanup?.();
      eff.cleanup = eff.fn();
    }
  }
  return el;
}

function unmountProvider() {
  for (const eff of hookEnv.effects) {
    eff.cleanup?.();
    eff.cleanup = undefined;
  }
}

describe("ErrorNotificationProvider", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    hookEnv.states = [];
    hookEnv.refs = [];
    hookEnv.effects = [];
    hookEnv.memos = [];
  });

  afterEach(() => {
    unmountProvider();
    vi.useRealTimers();
  });

  it("keeps contextValue referentially stable across notification updates and auto-dismisses", () => {
    const firstRender = renderProvider();
    const firstCtx = firstRender.props.value;

    firstCtx.showSuccess("Saved", 4000);
    const secondRender = renderProvider();
    expect(secondRender.props.value).toBe(firstCtx);
    expect(secondRender.props.children[1]).toBeTruthy();
    expect(vi.getTimerCount()).toBe(1);

    vi.advanceTimersByTime(4000);
    const afterTimeout = renderProvider();
    expect(afterTimeout.props.value).toBe(firstCtx);
    expect(afterTimeout.props.children[1]).toBeFalsy();
  });

  it("clears any pending auto-dismiss timeout on unmount", () => {
    const tree = renderProvider();
    tree.props.value.showError("RPC failed", "error", 8000);
    expect(vi.getTimerCount()).toBe(1);

    unmountProvider();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps notifications open when duration is 0 or negative until cleared manually", () => {
    const tree = renderProvider();
    tree.props.value.showWarning("Manual dismiss required", 0);
    expect(vi.getTimerCount()).toBe(0);

    let rendered = renderProvider();
    expect(rendered.props.children[1]).toBeTruthy();

    tree.props.value.clearNotification();
    rendered = renderProvider();
    expect(rendered.props.children[1]).toBeFalsy();
  });
});
