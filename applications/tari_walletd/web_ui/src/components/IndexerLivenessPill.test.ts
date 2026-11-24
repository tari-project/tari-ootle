// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { describe, expect, it } from "vitest";
import {
  failProbeStatus,
  formatLivenessTooltip,
  startProbeStatus,
  type IndexerLivenessStatus,
} from "./IndexerLivenessPill";

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

describe("failProbeStatus", () => {
  it("stays unknown on the first failure and flips to disconnected on the second", () => {
    const prev: IndexerLivenessStatus = {
      liveness: "connected",
      epoch: 42n,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    };
    const first = failProbeStatus(prev, "https://indexer-a.example", 1, new Error("timeout"));
    expect(first).toEqual({
      liveness: "unknown",
      epoch: null,
      indexerUrl: "https://indexer-a.example",
      detail: "timeout",
    });

    const second = failProbeStatus(first, "https://indexer-a.example", 2, "non-error");
    expect(second).toEqual({
      liveness: "disconnected",
      epoch: null,
      indexerUrl: "https://indexer-a.example",
      detail: "Cannot reach the indexer",
    });
  });

  it("clears stale indexerUrl when settings reports no configured indexer", () => {
    const prev: IndexerLivenessStatus = {
      liveness: "connected",
      epoch: 10n,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    };
    const next = failProbeStatus(prev, null, 1, new Error("No indexer URL is configured"));
    expect(next.indexerUrl).toBeNull();
    expect(formatLivenessTooltip(next)).toBe("No indexer configured · No indexer URL is configured");
  });

  it("preserves previous indexerUrl when settingsGet fails before resolving a URL", () => {
    const prev: IndexerLivenessStatus = {
      liveness: "connected",
      epoch: 10n,
      indexerUrl: "https://indexer-a.example",
      detail: null,
    };
    const next = failProbeStatus(prev, undefined, 1, new Error("RPC failed"));
    expect(next.indexerUrl).toBe("https://indexer-a.example");
    expect(next.epoch).toBeNull();
  });
});
