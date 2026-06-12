//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import {useCallback, useEffect, useMemo, useRef, useState} from "react";
import Box from "@mui/material/Box";
import Card from "@mui/material/Card";
import CardContent from "@mui/material/CardContent";
import Chip from "@mui/material/Chip";
import Grid from "@mui/material/Grid";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type {
  Epoch,
  ShardGroup,
  ValidatorConsensusState,
  ValidatorInfo,
  ValidatorStatus,
} from "@tari-project/ootle-ts-bindings";
import PageHeading from "../../Components/PageHeading";
import {StyledPaper} from "../../Components/StyledComponents";
import CopyToClipboard from "../../Components/CopyToClipboard";
import {shortenString} from "../VN/Components/helpers";
import {getNetworkStats, listValidators} from "../../utils/api";

const REFRESH_INTERVAL_MS = 5000;

type StateColour = "success" | "warning" | "error" | "info" | "default";

function stateColour(state: ValidatorConsensusState): StateColour {
  switch (state) {
    case "Running":
      return "success";
    case "CheckSync":
    case "Syncing":
      return "info";
    case "Idle":
    case "Sleeping":
      return "warning";
    case "Shutdown":
      return "error";
    default:
      return "default";
  }
}

function formatShardGroup(sg: ShardGroup): string {
  if (sg.start === sg.end_inclusive) {
    return `Shard ${sg.start}`;
  }
  return `Shards ${sg.start}-${sg.end_inclusive}`;
}

function compareShardGroups(a: ShardGroup, b: ShardGroup): number {
  if (a.start !== b.start) {
    return a.start - b.start;
  }
  return a.end_inclusive - b.end_inclusive;
}

function formatAge(seconds: number): string {
  if (seconds < 5) {
    return "just now";
  }
  if (seconds < 60) {
    return `${seconds}s ago`;
  }
  const mins = Math.floor(seconds / 60);
  if (mins < 60) {
    return `${mins}m ${seconds % 60}s ago`;
  }
  const hours = Math.floor(mins / 60);
  return `${hours}h ${mins % 60}m ago`;
}

interface RosterEntry {
  validator: ValidatorInfo;
  status: ValidatorStatus | null;
}

function ValidatorCard({ entry, nowSec }: { entry: RosterEntry; nowSec: number }) {
  const { validator, status } = entry;
  const ageSec = status ? Math.max(0, nowSec - Number(status.observed_at_unix_s)) : null;

  return (
    <Card
      variant="outlined"
      sx={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <CardContent>
        <Stack direction="row" justifyContent="space-between" alignItems="center" spacing={1}>
          {status ? (
            <Chip
              label={status.state}
              color={stateColour(status.state)}
              size="small"
              sx={{ fontWeight: 600, textTransform: "uppercase", letterSpacing: 0.5 }}
            />
          ) : (
            <Chip
              label="Not observed"
              size="small"
              sx={{ fontWeight: 600, textTransform: "uppercase", letterSpacing: 0.5 }}
            />
          )}
          {ageSec !== null && (
            <Typography variant="caption" color="text.secondary">
              {formatAge(ageSec)}
            </Typography>
          )}
        </Stack>

        <Box mt={2}>
          <Typography variant="overline" color="text.secondary">
            Public Key
          </Typography>
          <Stack direction="row" alignItems="center" spacing={0.5}>
            <Typography variant="body2" sx={{ fontFamily: "'Courier New', Courier, monospace" }}>
              {shortenString(validator.public_key)}
            </Typography>
            <CopyToClipboard copy={validator.public_key} />
          </Stack>
        </Box>

        <Box mt={1.5}>
          <Typography variant="overline" color="text.secondary">
            Peer ID
          </Typography>
          <Stack direction="row" alignItems="center" spacing={0.5}>
            <Typography variant="body2" sx={{ fontFamily: "'Courier New', Courier, monospace" }}>
              {shortenString(validator.peer_id)}
            </Typography>
            <CopyToClipboard copy={validator.peer_id} />
          </Stack>
        </Box>

        <Box mt={1.5}>
          <Typography variant="overline" color="text.secondary">
            {formatShardGroup(validator.shard_group)}
          </Typography>
        </Box>

        <Stack direction="row" spacing={3} mt={1.5}>
          <Box>
            <Typography variant="caption" color="text.secondary">
              Start Epoch
            </Typography>
            <Typography variant="h6">{String(validator.start_epoch)}</Typography>
          </Box>
          <Box>
            <Typography variant="caption" color="text.secondary">
              Vote Power
            </Typography>
            <Typography variant="h6">{String(validator.vote_power)}</Typography>
          </Box>
          {status && (
            <Box>
              <Typography variant="caption" color="text.secondary">
                Height
              </Typography>
              <Typography variant="h6">{String(status.height)}</Typography>
            </Box>
          )}
        </Stack>

        {validator.end_epoch !== null && (
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 1 }}>
            Deactivates at epoch {String(validator.end_epoch)}
          </Typography>
        )}
      </CardContent>
    </Card>
  );
}

function Validators() {
  const [epoch, setEpoch] = useState<Epoch | null>(null);
  const [roster, setRoster] = useState<ValidatorInfo[] | null>(null);
  const [statuses, setStatuses] = useState<ValidatorStatus[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [nowSec, setNowSec] = useState<number>(() => Math.floor(Date.now() / 1000));
  const mountedRef = useRef(false);

  const fetch = useCallback(async () => {
    try {
      const validatorsResp = await listValidators();
      setEpoch(validatorsResp.epoch);
      setRoster(validatorsResp.validators);
      setError(null);
    } catch (e) {
      console.error(e);
      setError("Failed to load validators");
      return;
    }
    try {
      const statsResp = await getNetworkStats();
      setStatuses(statsResp.validators);
    } catch (e) {
      // The roster renders without liveness snapshots; just log
      console.error(e);
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    fetch();
    const poll = window.setInterval(() => {
      if (mountedRef.current) {
        fetch();
      }
    }, REFRESH_INTERVAL_MS);
    const tick = window.setInterval(() => {
      if (mountedRef.current) {
        setNowSec(Math.floor(Date.now() / 1000));
      }
    }, 1000);
    return () => {
      mountedRef.current = false;
      window.clearInterval(poll);
      window.clearInterval(tick);
    };
  }, [fetch]);

  const entries = useMemo(() => {
    if (!roster) return null;
    const statusByPeer = new Map(statuses.map((s) => [s.peer_id, s]));
    return roster
      .map((validator) => ({
        validator,
        status: statusByPeer.get(validator.peer_id) ?? null,
      }))
      .sort((a, b) => {
        const groupCmp = compareShardGroups(a.validator.shard_group, b.validator.shard_group);
        if (groupCmp !== 0) return groupCmp;
        return a.validator.public_key.localeCompare(b.validator.public_key);
      });
  }, [roster, statuses]);

  return (
    <>
      <Grid size={12}>
        <PageHeading>Validators</PageHeading>
      </Grid>
      <Grid size={12}>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
          The full validator roster for the current epoch as tracked by the epoch manager. The indexer also
          periodically syncs state from random validators and records their self-reported (unverified) consensus
          status; where a snapshot exists for a validator, its last known status and the snapshot age are shown.
        </Typography>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          {error ? (
            <Typography color="error">{error}</Typography>
          ) : entries === null ? (
            <Typography color="text.secondary">Loading…</Typography>
          ) : entries.length === 0 ? (
            <Typography color="text.secondary">No validators are registered for the current epoch.</Typography>
          ) : (
            <>
              <Stack direction="row" spacing={1} alignItems="center" sx={{ mb: 2 }}>
                {epoch !== null && <Chip label={`Epoch ${epoch}`} size="small" />}
                <Chip label={`${entries.length} validator${entries.length === 1 ? "" : "s"}`} size="small" />
              </Stack>
              <Grid container spacing={2}>
                {entries.map((entry) => (
                  <Grid key={entry.validator.public_key} size={{ xs: 12, sm: 6, md: 4, lg: 3 }}>
                    <ValidatorCard entry={entry} nowSec={nowSec} />
                  </Grid>
                ))}
              </Grid>
            </>
          )}
        </StyledPaper>
      </Grid>
    </>
  );
}

export default Validators;
