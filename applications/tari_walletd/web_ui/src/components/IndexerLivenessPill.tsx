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

import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import Tooltip from "@mui/material/Tooltip";
import { useTheme } from "@mui/material/styles";
import useAuthStore from "@store/authStore";
import { indexerGetNetworkInfo, settingsGet } from "@utils/json_rpc";
import { useEffect, useRef, useState } from "react";

/// How often the configured indexer is re-probed.
const POLL_INTERVAL_MS = 15_000;

/// Consecutive failures tolerated before the pill goes red. One failed probe leaves the pill
/// unknown so a single dropped request does not read as an outage.
const FAILURES_BEFORE_DISCONNECTED = 2;

/// "connecting" is a request in flight with no verdict yet; "unknown" is no verdict and nothing in
/// flight, which is also where a probe that failed once lands.
type Liveness = "connected" | "connecting" | "unknown" | "disconnected";

interface Status {
  liveness: Liveness;
  epoch: bigint | number | null;
  indexerUrl: string | null;
  detail: string | null;
}

const LABELS: Record<Liveness, string> = {
  connected: "Connected",
  connecting: "Connecting…",
  unknown: "Unknown",
  disconnected: "Disconnected",
};

/**
 * Liveness of the indexer this wallet is configured to use, as a coloured pill.
 *
 * Probes the same endpoint the indexer settings tab uses, reading the URL from `settings.get` so
 * that changing the indexer takes effect here without a reload. The probe goes browser → indexer
 * directly, so it reflects whether *this* page can reach the indexer.
 *
 * `settings.get` is authenticated, so the pill is inert until the user is logged in — probing
 * before then would only ever report the authentication failure as an indexer outage.
 */
function IndexerLivenessPill() {
  const theme = useTheme();
  const loggedIn = useAuthStore((state) => state.loggedIn);
  const [status, setStatus] = useState<Status>({
    liveness: "unknown",
    epoch: null,
    indexerUrl: null,
    detail: null,
  });
  // Counted across polls rather than held in state, so a re-probe does not re-render the pill
  // before it has a verdict.
  const consecutiveFailures = useRef(0);

  useEffect(() => {
    if (!loggedIn) {
      // A logged out session carries no probe history into the next one.
      consecutiveFailures.current = 0;
      setStatus({ liveness: "unknown", epoch: null, indexerUrl: null, detail: null });
      return;
    }

    let cancelled = false;

    const probe = async () => {
      try {
        const settings = await settingsGet();
        if (cancelled) return;
        const indexerUrl = settings.indexer_url;
        if (!indexerUrl) {
          throw new Error("No indexer URL is configured");
        }
        // Announced only when there is no verdict to keep showing, so a poll from an established
        // state does not flicker through "Connecting…" every interval.
        setStatus((prev) => (prev.liveness === "unknown" ? { ...prev, liveness: "connecting", indexerUrl } : prev));
        const info = await indexerGetNetworkInfo(indexerUrl);
        if (cancelled) return;
        consecutiveFailures.current = 0;
        setStatus({
          liveness: "connected",
          epoch: info.epoch,
          indexerUrl,
          detail: null,
        });
      } catch (e) {
        if (cancelled) return;
        consecutiveFailures.current += 1;
        const detail = e instanceof Error ? e.message : "Cannot reach the indexer";
        setStatus((prev) => ({
          ...prev,
          liveness: consecutiveFailures.current >= FAILURES_BEFORE_DISCONNECTED ? "disconnected" : "unknown",
          detail,
        }));
      }
    };

    void probe();
    const timer = setInterval(() => void probe(), POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [loggedIn]);

  if (!loggedIn) {
    return null;
  }

  const colour = {
    connected: theme.palette.success.main,
    connecting: theme.palette.info.main,
    unknown: theme.palette.text.disabled,
    disconnected: theme.palette.error.main,
  }[status.liveness];

  const tooltip = [
    status.indexerUrl ? `Indexer: ${status.indexerUrl}` : "No indexer configured",
    status.liveness === "connected" && status.epoch != null ? `Epoch: ${status.epoch.toString()}` : null,
    status.detail,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <Tooltip title={tooltip} arrow>
      <Chip
        size="small"
        variant="outlined"
        label={LABELS[status.liveness]}
        icon={
          <Box
            component="span"
            sx={{
              width: 8,
              height: 8,
              borderRadius: "50%",
              backgroundColor: colour,
              marginLeft: "8px !important",
            }}
          />
        }
        sx={{
          borderColor: colour,
          color: theme.palette.text.secondary,
        }}
      />
    </Tooltip>
  );
}

export default IndexerLivenessPill;
