//  Copyright 2026. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

/// How often the configured indexer is re-probed after the previous probe settles.
export const POLL_INTERVAL_MS = 15_000;

/// How long each request in a probe may stay in flight before it counts as a failure.
export const PROBE_TIMEOUT_MS = 10_000;

/// Consecutive failures tolerated before the pill goes red. One failed probe leaves the pill
/// unknown so a single dropped request does not read as an outage.
export const FAILURES_BEFORE_DISCONNECTED = 2;

/// "connecting" is a request in flight with no verdict yet; "unknown" is no verdict and nothing in
/// flight, which is also where a probe that failed once lands.
export type Liveness = "connected" | "connecting" | "unknown" | "disconnected";

export interface IndexerLivenessStatus {
  liveness: Liveness;
  epoch: bigint | number | null;
  indexerUrl: string | null;
  detail: string | null;
}

export const INITIAL_LIVENESS_STATUS: IndexerLivenessStatus = {
  liveness: "unknown",
  epoch: null,
  indexerUrl: null,
  detail: null,
};

export function startProbeStatus(prev: IndexerLivenessStatus, indexerUrl: string): IndexerLivenessStatus {
  if (prev.indexerUrl !== indexerUrl) {
    return {
      liveness: "connecting",
      epoch: null,
      indexerUrl,
      detail: null,
    };
  }
  return prev.liveness === "unknown" ? { ...prev, liveness: "connecting" } : prev;
}

export function failProbeStatus(
  prev: IndexerLivenessStatus,
  resolvedUrl: string | undefined,
  consecutiveFailures: number,
  error: unknown,
): IndexerLivenessStatus {
  const detail = error instanceof Error ? error.message : "Cannot reach the indexer";
  return {
    liveness: consecutiveFailures >= FAILURES_BEFORE_DISCONNECTED ? "disconnected" : "unknown",
    epoch: null,
    indexerUrl: resolvedUrl ?? prev.indexerUrl,
    detail,
  };
}

export function formatLivenessTooltip(status: IndexerLivenessStatus): string {
  return [
    status.indexerUrl ? `Indexer: ${status.indexerUrl}` : "No indexer configured",
    status.liveness === "connected" && status.epoch != null ? `Epoch: ${status.epoch.toString()}` : null,
    status.detail,
  ]
    .filter(Boolean)
    .join(" · ");
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), timeoutMs);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}

export interface IndexerLivenessPollerOptions {
  settingsGet: () => Promise<{ indexer_url: string }>;
  indexerGetNetworkInfo: (indexerUrl: string) => Promise<{ epoch: bigint | number }>;
  onStatus: (updater: (prev: IndexerLivenessStatus) => IndexerLivenessStatus) => void;
  pollIntervalMs?: number;
  probeTimeoutMs?: number;
}

export function startIndexerLivenessPoller({
  settingsGet,
  indexerGetNetworkInfo,
  onStatus,
  pollIntervalMs = POLL_INTERVAL_MS,
  probeTimeoutMs = PROBE_TIMEOUT_MS,
}: IndexerLivenessPollerOptions): () => void {
  let cancelled = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let consecutiveFailures = 0;
  let lastIndexerUrl: string | null = null;

  const scheduleNext = () => {
    if (!cancelled) {
      timer = setTimeout(() => void probe(), pollIntervalMs);
    }
  };

  const probe = async () => {
    let resolvedUrl: string | undefined = undefined;
    try {
      const settings = await withTimeout(settingsGet(), probeTimeoutMs, "Wallet settings request timed out");
      if (cancelled) return;
      const indexerUrl = settings.indexer_url;
      if (!indexerUrl) {
        throw new Error("No indexer URL is configured");
      }
      resolvedUrl = indexerUrl;
      if (lastIndexerUrl !== indexerUrl) {
        lastIndexerUrl = indexerUrl;
        consecutiveFailures = 0;
      }
      // Announced only when there is no verdict to keep showing (or the URL changed), so a poll
      // from an established state does not flicker through "Connecting…" every interval.
      onStatus((prev) => startProbeStatus(prev, indexerUrl));
      const info = await withTimeout(indexerGetNetworkInfo(indexerUrl), probeTimeoutMs, "Indexer did not respond");
      if (cancelled) return;
      consecutiveFailures = 0;
      onStatus(() => ({
        liveness: "connected",
        epoch: info.epoch,
        indexerUrl,
        detail: null,
      }));
    } catch (e) {
      if (cancelled) return;
      consecutiveFailures += 1;
      const failures = consecutiveFailures;
      onStatus((prev) => failProbeStatus(prev, resolvedUrl, failures, e));
    } finally {
      scheduleNext();
    }
  };

  void probe();
  return () => {
    cancelled = true;
    if (timer !== null) {
      clearTimeout(timer);
    }
  };
}
