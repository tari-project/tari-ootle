// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { QueryClient, QueryObserver, type UseQueryOptions } from "@tanstack/react-query";
import { encodeOotleAddress } from "@tari-project/ootle-ts-bindings";
import { ChangeEvent, isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

interface ComponentInstance {
  key: string | null;
  states: unknown[];
  cursor: number;
  observers: Array<{ observer: QueryObserver<any, any, any, any, any>; unsubscribe: () => void }>;
}

const harness = {
  queryClient: new QueryClient({ defaultOptions: { queries: { retry: false } } }),
  instances: new Map<string, ComponentInstance>(),
  current: null as ComponentInstance | null,
};

const { mockTemplatesListAuthored, accountStoreState } = vi.hoisted(() => ({
  mockTemplatesListAuthored: vi.fn(),
  accountStoreState: { address: undefined as string | undefined },
}));

vi.mock("@utils/json_rpc", () => ({
  templatesListAuthored: mockTemplatesListAuthored,
}));

vi.mock("@store/accountStore", () => {
  const useAccountStore = (selector: (state: { address?: string }) => unknown) => selector(accountStoreState);
  useAccountStore.setState = (partial: { address?: string }) => {
    Object.assign(accountStoreState, partial);
  };
  return { default: useAccountStore };
});

vi.mock("react", async () => {
  const actual = await vi.importActual<typeof import("react")>("react");
  return {
    ...actual,
    useState<T>(initial: T | (() => T)) {
      const inst = harness.current;
      if (!inst) {
        return actual.useState(initial);
      }
      const idx = inst.cursor++;
      if (!(idx in inst.states)) {
        inst.states[idx] = typeof initial === "function" ? (initial as () => T)() : initial;
      }
      const setState = (next: T | ((prev: T) => T)) => {
        inst.states[idx] = typeof next === "function" ? (next as (prev: T) => T)(inst.states[idx] as T) : next;
      };
      return [inst.states[idx] as T, setState] as const;
    },
  };
});

vi.mock("@tanstack/react-query", async () => {
  const actual = await vi.importActual<typeof import("@tanstack/react-query")>("@tanstack/react-query");
  return {
    ...actual,
    useQuery(options: UseQueryOptions<any, any, any, any>) {
      const inst = harness.current!;
      const defaulted = harness.queryClient.defaultQueryOptions(options);
      const observer = new actual.QueryObserver(harness.queryClient, defaulted);
      const unsubscribe = observer.subscribe(() => {});
      inst.observers.push({ observer, unsubscribe });
      return observer.getOptimisticResult(defaulted);
    },
  };
});

import useAccountStore from "@store/accountStore";
import TemplateList from "./TemplateList";

function unmountInstance(slot: string) {
  const existing = harness.instances.get(slot);
  if (existing) {
    for (const entry of existing.observers) {
      entry.unsubscribe();
    }
    harness.instances.delete(slot);
  }
}

function renderComponent<P>(slot: string, Component: (props: P) => ReactNode, props: P, key: string | null): ReactNode {
  let inst = harness.instances.get(slot);
  if (!inst || inst.key !== key) {
    unmountInstance(slot);
    inst = { key, states: [], cursor: 0, observers: [] };
    harness.instances.set(slot, inst);
  } else {
    for (const entry of inst.observers) {
      entry.unsubscribe();
    }
    inst.observers = [];
    inst.cursor = 0;
  }
  harness.current = inst;
  try {
    return Component(props);
  } finally {
    harness.current = null;
  }
}

function renderTemplateList() {
  const outer = renderComponent("root", TemplateList, {}, null) as ReactElement<any>;
  const contentEl = renderComponent("content", outer.type as (props: any) => ReactNode, outer.props, outer.key);
  return { outer, contentEl };
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

function makeOotleAddress(fillByte: string) {
  const accountPublicKey = fillByte.repeat(32);
  const viewOnlyKey = "22".repeat(32);
  return {
    accountPublicKey,
    address: encodeOotleAddress({
      network: "localnet",
      accountPublicKey,
      viewOnlyKey,
      destinationTag: null,
    }),
  };
}

describe("TemplateList", () => {
  beforeEach(() => {
    harness.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    harness.instances.clear();
    mockTemplatesListAuthored.mockReset();
    useAccountStore.setState({ address: undefined });
  });

  afterEach(() => {
    for (const slot of Array.from(harness.instances.keys())) {
      unmountInstance(slot);
    }
    harness.queryClient.clear();
  });

  it("does not invoke templatesListAuthored while the account address is missing or malformed", () => {
    renderTemplateList();
    expect(mockTemplatesListAuthored).not.toHaveBeenCalled();

    useAccountStore.setState({ address: "not-a-valid-ootle-address" });
    renderTemplateList();
    expect(mockTemplatesListAuthored).not.toHaveBeenCalled();
  });

  it("resets page and expanded row on account switch while preserving rowsPerPage", async () => {
    const accountA = makeOotleAddress("aa");
    const accountB = makeOotleAddress("bb");

    mockTemplatesListAuthored.mockResolvedValue({
      templates: [{ address: "tpl_1", name: "Counter", abi_version: 1, functions: [] }],
      total_templates: 100n,
    });

    useAccountStore.setState({ address: accountA.address });
    let { contentEl } = renderTemplateList();
    await vi.waitFor(() => {
      expect(mockTemplatesListAuthored).toHaveBeenCalledWith({
        author_public_key: accountA.accountPublicKey,
        page: 0,
        page_size: 10,
      });
    });

    // Wait for the resolved query data to enter the QueryClient cache and expand the first template row
    let expandButtons: ReactElement<any>[] = [];
    await vi.waitFor(() => {
      ({ contentEl } = renderTemplateList());
      expandButtons = findElements(contentEl, (el) => el.props?.["aria-label"] === "expand row");
      expect(expandButtons).toHaveLength(1);
    });
    expandButtons[0].props.onClick();

    // Change rowsPerPage to 25 and navigate to page 2
    const pagination = findElements(contentEl, (el) => typeof el.props?.onPageChange === "function")[0];
    pagination.props.onRowsPerPageChange({
      target: { value: "25" },
    } as ChangeEvent<HTMLInputElement>);
    ({ contentEl } = renderTemplateList());
    const paginationAfterRows = findElements(contentEl, (el) => typeof el.props?.onPageChange === "function")[0];
    paginationAfterRows.props.onPageChange(null, 2);
    ({ contentEl } = renderTemplateList());

    await vi.waitFor(() => {
      expect(mockTemplatesListAuthored).toHaveBeenCalledWith({
        author_public_key: accountA.accountPublicKey,
        page: 2,
        page_size: 25,
      });
    });

    await vi.waitFor(() => {
      ({ contentEl } = renderTemplateList());
      expect(findElements(contentEl, (el) => el.props?.isOpen === true)).toHaveLength(1);
    });

    // Switch to account B: page must reset synchronously to 0, openItem collapses, rowsPerPage stays 25
    mockTemplatesListAuthored.mockClear();
    useAccountStore.setState({ address: accountB.address });
    ({ contentEl } = renderTemplateList());

    const paginationB = findElements(contentEl, (el) => typeof el.props?.onPageChange === "function")[0];
    expect(paginationB.props.page).toBe(0);
    expect(paginationB.props.rowsPerPage).toBe(25);
    expect(findElements(contentEl, (el) => el.props?.isOpen === true)).toHaveLength(0);

    await vi.waitFor(() => {
      expect(mockTemplatesListAuthored).toHaveBeenCalledTimes(1);
      expect(mockTemplatesListAuthored).toHaveBeenCalledWith({
        author_public_key: accountB.accountPublicKey,
        page: 0,
        page_size: 25,
      });
    });
  });
});
