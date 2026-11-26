// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  failProbeStatus,
  formatLivenessTooltip,
  INITIAL_LIVENESS_STATUS,
  POLL_INTERVAL_MS,
  PROBE_TIMEOUT_MS,
  startIndexerLivenessPoller,
  startProbeStatus,
  type IndexerLivenessStatus,
} from "./indexerLivenessPoller";

describe("startProbeStatus", () => {
  it("transitions from unknown to connecting for the same indexer URL", () => {
    const prev: IndexerLivenessStatus = {
      liveness: "unknown",
      epoch: null,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    };
    expect(startProbeStatus(prev, "https://indexer-a.example")).toEqual({
      liveness: "connecting",
      epoch: null,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    });
  });

  it("keeps an established connected state without flickering on re-probe of the same URL", () => {
    const prev: IndexerLivenessStatus = {
      liveness: "connected",
      epoch: 42n,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    };
    expect(startProbeStatus(prev, "https://indexer-a.example")).toBe(prev);
  });

  it("resets stale epoch and enters connecting when the configured indexer URL changes", () => {
    const prev: IndexerLivenessStatus = {
      liveness: "connected",
      epoch: 42n,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    };
    expect(startProbeStatus(prev, "https://indexer-b.example")).toEqual({
      liveness: "connecting",
      epoch: null,
      indexerUrl: "https://indexer-b.example",
      detail: null,
    });
  });
});

describe("failProbeStatus & formatLivenessTooltip", () => {
  it("preserves previous indexerUrl when settingsGet fails before resolving a URL", () => {
    const prev: IndexerLivenessStatus = {
      liveness: "connected",
      epoch: 10n,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    };
    const next = failProbeStatus(prev, undefined, 1, new Error("RPC failed"));
    expect(next).toEqual({
      liveness: "unknown",
      epoch: null,
      indexerUrl: "https://indexer-a.example",
      detail: "RPC failed",
    });
    expect(formatLivenessTooltip(next)).toBe("Indexer: https://indexer-a.example · RPC failed");
  });
});

describe("startIndexerLivenessPoller", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("runs only one probe at a time and schedules the next probe after completion", async () => {
    let resolveProbe!: (value: { epoch: number }) => void;
    const settingsGet = vi.fn().mockResolvedValue({ indexer_url: "https://indexer-a.example" });
    const indexerGetNetworkInfo = vi.fn(
      () =>
        new Promise<{ epoch: number }>((resolve) => {
          resolveProbe = resolve;
        }),
    );

    let current: IndexerLivenessStatus = INITIAL_LIVENESS_STATUS;
    const stop = startIndexerLivenessPoller({
      settingsGet,
      indexerGetNetworkInfo,
      onStatus: (updater) => {
        current = updater(current);
      },
      pollIntervalMs: POLL_INTERVAL_MS,
      probeTimeoutMs: 60_000,
    });

    await vi.advanceTimersByTimeAsync(0);
    expect(indexerGetNetworkInfo).toHaveBeenCalledTimes(1);
    expect(current.liveness).toBe("connecting");

    // Advancing past the poll interval while the first probe is still pending must not start a second probe.
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS * 2);
    expect(indexerGetNetworkInfo).toHaveBeenCalledTimes(1);

    resolveProbe({ epoch: 7 });
    await vi.advanceTimersByTimeAsync(0);
    expect(current).toEqual({
      liveness: "connected",
      epoch: 7,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    });

    // Next probe fires only after POLL_INTERVAL_MS elapses from completion.
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS - 1);
    expect(indexerGetNetworkInfo).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(indexerGetNetworkInfo).toHaveBeenCalledTimes(2);

    stop();
  });

  it("times out a hanging probe below POLL_INTERVAL_MS and continues polling", async () => {
    const settingsGet = vi.fn().mockResolvedValue({ indexer_url: "https://indexer-a.example" });
    const indexerGetNetworkInfo = vi
      .fn()
      .mockImplementationOnce(() => new Promise<{ epoch: number }>(() => {}))
      .mockResolvedValueOnce({ epoch: 99 });

    let current: IndexerLivenessStatus = INITIAL_LIVENESS_STATUS;
    const stop = startIndexerLivenessPoller({
      settingsGet,
      indexerGetNetworkInfo,
      onStatus: (updater) => {
        current = updater(current);
      },
      pollIntervalMs: POLL_INTERVAL_MS,
      probeTimeoutMs: PROBE_TIMEOUT_MS,
    });

    await vi.advanceTimersByTimeAsync(PROBE_TIMEOUT_MS);
    expect(current).toEqual({
      liveness: "unknown",
      epoch: null,
      indexerUrl: "https://indexer-a.example",
      detail: "Indexer did not respond",
    });

    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS);
    expect(indexerGetNetworkInfo).toHaveBeenCalledTimes(2);
    expect(current).toEqual({
      liveness: "connected",
      epoch: 99,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    });

    stop();
  });

  it("resets consecutive failure count when the configured indexer URL changes", async () => {
    let activeUrl = "https://indexer-a.example";
    const settingsGet = vi.fn(async () => ({ indexer_url: activeUrl }));
    const indexerGetNetworkInfo = vi.fn().mockRejectedValue(new Error("connection refused"));

    let current: IndexerLivenessStatus = INITIAL_LIVENESS_STATUS;
    const stop = startIndexerLivenessPoller({
      settingsGet,
      indexerGetNetworkInfo,
      onStatus: (updater) => {
        current = updater(current);
      },
      pollIntervalMs: POLL_INTERVAL_MS,
      probeTimeoutMs: PROBE_TIMEOUT_MS,
    });

    // First failure on indexer-a -> unknown
    await vi.advanceTimersByTimeAsync(0);
    expect(current.liveness).toBe("unknown");
    expect(current.indexerUrl).toBe("https://indexer-a.example");

    // Switch to indexer-b before the next poll: failure count must reset so 1st failure stays unknown.
    activeUrl = "https://indexer-b.example";
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS);
    expect(current.liveness).toBe("unknown");
    expect(current.indexerUrl).toBe("https://indexer-b.example");

    // Second consecutive failure on indexer-b -> disconnected.
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS);
    expect(current.liveness).toBe("disconnected");
    expect(current.indexerUrl).toBe("https://indexer-b.example");

    stop();
  });
});
