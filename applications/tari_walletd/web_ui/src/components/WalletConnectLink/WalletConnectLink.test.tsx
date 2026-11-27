// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Minimal hook harness, as in CopyToClipboard.test.tsx: tari_walletd/web_ui runs Vitest in the Node environment.
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

const walletKitInit = vi.hoisted(() => vi.fn());

vi.mock("@reown/walletkit", () => ({ WalletKit: { init: walletKitInit } }));
vi.mock("@walletconnect/core", () => ({ Core: class {} }));
vi.mock("@walletconnect/utils", () => ({
  buildApprovedNamespaces: () => ({ tari: {} }),
  getSdkError: (key: string) => ({ code: 0, message: key }),
}));
vi.mock("@store/accountStore", () => ({ default: () => ({ account: undefined }) }));
vi.mock("@utils/json_rpc", () => ({
  accountsCreateFreeTestCoins: vi.fn(),
  accountsGet: vi.fn(),
  accountsGetBalances: vi.fn(),
  accountsGetDefault: vi.fn(),
  accountsList: vi.fn(),
  confidentialViewVaultBalance: vi.fn(),
  nftList: vi.fn(),
  substatesGet: vi.fn(),
  substatesList: vi.fn(),
  templatesGet: vi.fn(),
  transactionsGetResult: vi.fn(),
  transactionsSubmit: vi.fn(),
  walletGetInfo: vi.fn(),
}));
vi.mock("@components/WalletConnectLink/CheckMark", () => ({ default: () => null }));
vi.mock("@components/WalletConnectLink/ConfirmTransaction", () => ({ default: () => null }));
vi.mock("@components/WalletConnectLink/ConnectorLogo", () => ({ default: () => null }));
vi.mock("@components/WalletConnectLink/Permissions", () => ({ default: () => null }));
vi.mock("@mui/icons-material", () => ({ Error: () => null }));
vi.mock("@mui/icons-material/Close", () => ({ default: () => null }));
vi.mock("@mui/material/styles", () => ({
  useTheme: () => ({ breakpoints: { up: () => "" }, palette: { text: { primary: "#000" } } }),
}));
vi.mock("@mui/material/useMediaQuery", () => ({ default: () => true }));
vi.mock("./ConnectorLink.css", () => ({}));

import ConnectorDialog from "./WalletConnectLink";

function render(): ReactElement<any> {
  hookEnv.stateCursor = 0;
  hookEnv.refCursor = 0;
  hookEnv.effectCursor = 0;
  const el = ConnectorDialog() as ReactElement<any>;
  for (const eff of hookEnv.effects) {
    if (eff.pending) {
      eff.pending = false;
      eff.cleanup?.();
      eff.cleanup = eff.fn();
    }
  }
  return el;
}

function textOf(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") {
    return String(node);
  }
  if (Array.isArray(node)) {
    return node.map(textOf).join("");
  }
  if (isValidElement(node)) {
    return textOf((node.props as { children?: ReactNode }).children);
  }
  return "";
}

function findButton(node: ReactNode, label: string): ReactElement<any> | undefined {
  if (Array.isArray(node)) {
    for (const child of node) {
      const found = findButton(child, label);
      if (found) {
        return found;
      }
    }
    return undefined;
  }
  if (!isValidElement(node)) {
    return undefined;
  }
  const props = node.props as { children?: ReactNode; onClick?: unknown };
  if (typeof props.onClick === "function" && textOf(props.children).trim() === label) {
    return node as ReactElement<any>;
  }
  return findButton(props.children, label);
}

async function click(label: string) {
  const button = findButton(render(), label);
  if (!button) {
    throw new Error(`No "${label}" button rendered`);
  }
  await button.props.onClick();
}

function closeDialog() {
  const children = render().props.children as ReactNode[];
  const dialog = children.find((child) => isValidElement(child) && "open" in (child.props as object));
  if (!isValidElement(dialog)) {
    throw new Error("No dialog rendered");
  }
  (dialog.props as { onClose: () => void }).onClose();
}

function fakeWallet() {
  const handlers: Record<string, (event: any) => Promise<void>> = {};
  return {
    handlers,
    on: vi.fn((event: string, handler: (event: any) => Promise<void>) => {
      handlers[event] = handler;
    }),
    pair: vi.fn().mockResolvedValue(undefined),
    approveSession: vi.fn().mockResolvedValue({ topic: "session-topic" }),
    respondSessionRequest: vi.fn().mockRejectedValue(new Error("No matching key")),
    rejectSession: vi.fn().mockResolvedValue(undefined),
  };
}

function tariProposal(id: number) {
  return {
    id,
    params: {
      optionalNamespaces: { tari: { chains: ["tari:devnet"] } },
      requiredNamespaces: {},
      sessionProperties: { required_permissions: "[]", optional_permissions: "[]" },
    },
  };
}

describe("WalletConnectLink", () => {
  const originalNavigator = globalThis.navigator;

  beforeEach(() => {
    vi.useFakeTimers();
    hookEnv.states = [];
    hookEnv.refs = [];
    hookEnv.effects = [];
    walletKitInit.mockReset();
    Object.defineProperty(globalThis, "navigator", {
      value: { clipboard: { readText: vi.fn().mockResolvedValue("wc:link") } },
      configurable: true,
      writable: true,
    });
  });

  afterEach(() => {
    for (const eff of hookEnv.effects) {
      eff.cleanup?.();
    }
    vi.useRealTimers();
    Object.defineProperty(globalThis, "navigator", { value: originalNavigator, configurable: true, writable: true });
    vi.restoreAllMocks();
  });

  it("approves with approveSession alone and ignores a stale proposal when pairing a second link", async () => {
    const wallet = fakeWallet();
    walletKitInit.mockResolvedValue(wallet);

    await click("Connect with WalletConnect");
    await click("Yes, Connect");
    expect(wallet.pair).toHaveBeenCalledTimes(1);

    await wallet.handlers.session_proposal(tariProposal(1));
    await click("Authorize");
    expect(wallet.approveSession).toHaveBeenCalledWith({ id: 1, namespaces: { tari: {} } });
    expect(wallet.respondSessionRequest).not.toHaveBeenCalled();
    expect(wallet.rejectSession).not.toHaveBeenCalled();
    expect(textOf(render())).toContain("Wallet Connected");

    closeDialog();
    vi.advanceTimersByTime(500);
    await click("Connect with WalletConnect");
    expect(textOf(render())).toContain("Yes, Connect");

    // Second link reuses the client; Authorize before its proposal arrives reports a missing proposal.
    await click("Yes, Connect");
    expect(walletKitInit).toHaveBeenCalledTimes(1);
    expect(wallet.pair).toHaveBeenCalledTimes(2);
    await click("Authorize");
    expect(wallet.approveSession).toHaveBeenCalledTimes(1);
    expect(wallet.rejectSession).not.toHaveBeenCalled();
    expect(textOf(render())).toContain("No active WalletConnect session proposal");

    await wallet.handlers.session_proposal(tariProposal(2));
    await click("Authorize");
    expect(wallet.approveSession).toHaveBeenLastCalledWith({ id: 2, namespaces: { tari: {} } });
    expect(textOf(render())).toContain("Wallet Connected");
  });

  it("stays on the authorize page and rejects the proposal when approveSession fails", async () => {
    const wallet = fakeWallet();
    wallet.approveSession.mockRejectedValue(new Error("approve failed"));
    walletKitInit.mockResolvedValue(wallet);

    await click("Connect with WalletConnect");
    await click("Yes, Connect");
    await wallet.handlers.session_proposal(tariProposal(7));
    await click("Authorize");

    expect(wallet.rejectSession).toHaveBeenCalledWith({ id: 7, reason: { code: 0, message: "USER_REJECTED" } });
    const tree = render();
    expect(textOf(tree)).toContain("approve failed");
    expect(findButton(tree, "Authorize")).toBeDefined();
  });
});
