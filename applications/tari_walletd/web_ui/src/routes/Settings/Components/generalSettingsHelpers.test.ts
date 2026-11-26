// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it, vi } from "vitest";
import { createSettingsLoader, formatSettingsError } from "./generalSettingsHelpers";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("formatSettingsError", () => {
  it("extracts Error messages and falls back cleanly for empty or unknown values", () => {
    expect(formatSettingsError(new Error("Connection refused"))).toBe("Connection refused");
    expect(formatSettingsError("RPC timeout")).toBe("RPC timeout");
    expect(formatSettingsError(new Error("   "))).toBe("Failed to load settings.");
    expect(formatSettingsError(null)).toBe("Failed to load settings.");
  });
});

describe("createSettingsLoader", () => {
  it("reports rejected settings requests via onError and clears error on retry", async () => {
    const first = deferred<{ indexer_url: string }>();
    const second = deferred<{ indexer_url: string }>();
    const fetchSettings = vi
      .fn<() => Promise<{ indexer_url: string }>>()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const onStart = vi.fn();
    const onSuccess = vi.fn();
    const onError = vi.fn();

    const loader = createSettingsLoader({ fetchSettings, onStart, onSuccess, onError });

    const p1 = loader.load();
    first.reject(new Error("Daemon offline"));
    await p1;

    expect(onStart).toHaveBeenCalledTimes(1);
    expect(onError).toHaveBeenCalledWith("Daemon offline");
    expect(onSuccess).not.toHaveBeenCalled();

    const p2 = loader.load();
    second.resolve({ indexer_url: "https://indexer.example" });
    await p2;

    expect(onStart).toHaveBeenCalledTimes(2);
    expect(onSuccess).toHaveBeenCalledWith({ indexer_url: "https://indexer.example" });
  });

  it("drops an older response or error that settles after a newer load", async () => {
    const older = deferred<{ indexer_url: string }>();
    const newer = deferred<{ indexer_url: string }>();
    const fetchSettings = vi
      .fn<() => Promise<{ indexer_url: string }>>()
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(newer.promise);
    const onSuccess = vi.fn();
    const onError = vi.fn();

    const loader = createSettingsLoader({ fetchSettings, onSuccess, onError });

    const pOld = loader.load();
    const pNew = loader.load();

    newer.resolve({ indexer_url: "https://new-indexer.example" });
    await pNew;

    older.resolve({ indexer_url: "https://stale-indexer.example" });
    await pOld;

    expect(onSuccess).toHaveBeenCalledTimes(1);
    expect(onSuccess).toHaveBeenCalledWith({ indexer_url: "https://new-indexer.example" });
    expect(onError).not.toHaveBeenCalled();
  });

  it("drops an older rejection that settles after a newer load succeeds", async () => {
    const older = deferred<{ indexer_url: string }>();
    const newer = deferred<{ indexer_url: string }>();
    const fetchSettings = vi
      .fn<() => Promise<{ indexer_url: string }>>()
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(newer.promise);
    const onSuccess = vi.fn();
    const onError = vi.fn();

    const loader = createSettingsLoader({ fetchSettings, onSuccess, onError });

    const pOld = loader.load();
    const pNew = loader.load();

    newer.resolve({ indexer_url: "https://new-indexer.example" });
    await pNew;

    older.reject(new Error("Stale timeout"));
    await pOld;

    expect(onSuccess).toHaveBeenCalledTimes(1);
    expect(onError).not.toHaveBeenCalled();
  });

  it("drops in-flight settlements after invalidate() on unmount", async () => {
    const pending = deferred<{ indexer_url: string }>();
    const onSuccess = vi.fn();
    const onError = vi.fn();

    const loader = createSettingsLoader({
      fetchSettings: () => pending.promise,
      onSuccess,
      onError,
    });

    const p = loader.load();
    loader.invalidate();

    pending.resolve({ indexer_url: "https://after-unmount.example" });
    await p;

    expect(onSuccess).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
  });
});
