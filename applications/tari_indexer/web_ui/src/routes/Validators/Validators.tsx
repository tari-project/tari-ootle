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
import type {ShardGroup, ValidatorConsensusState, ValidatorStatus} from "@tari-project/ootle-ts-bindings";
import PageHeading from "../../Components/PageHeading";
import {StyledPaper} from "../../Components/StyledComponents";
import CopyToClipboard from "../../Components/CopyToClipboard";
import {shortenString} from "../VN/Components/helpers";
import {getNetworkStats} from "../../utils/api";

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

function formatAge(seconds: number): string {
  if (seconds < 0) {
    return "just now";
  }
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

function ValidatorCard({ validator, nowSec }: { validator: ValidatorStatus; nowSec: number }) {
  const ageSec = Math.max(0, nowSec - Number(validator.observed_at_unix_s));

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
          <Chip
            label={validator.state}
            color={stateColour(validator.state)}
            size="small"
            sx={{ fontWeight: 600, textTransform: "uppercase", letterSpacing: 0.5 }}
          />
          <Typography variant="caption" color="text.secondary">
            {formatAge(ageSec)}
          </Typography>
        </Stack>

        <Box mt={2}>
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
              Epoch
            </Typography>
            <Typography variant="h6">{String(validator.epoch)}</Typography>
          </Box>
          <Box>
            <Typography variant="caption" color="text.secondary">
              Height
            </Typography>
            <Typography variant="h6">{String(validator.height)}</Typography>
          </Box>
        </Stack>
      </CardContent>
    </Card>
  );
}

function Validators() {
  const [validators, setValidators] = useState<ValidatorStatus[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nowSec, setNowSec] = useState<number>(() => Math.floor(Date.now() / 1000));
  const mountedRef = useRef(false);

  const fetch = useCallback(async () => {
    try {
      const resp = await getNetworkStats();
      setValidators(resp.validators);
      setError(null);
    } catch (e) {
      console.error(e);
      setError("Failed to load validator status");
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

  const sorted = useMemo(() => {
    if (!validators) return null;
    return [...validators].sort((a, b) => {
      const timeCmp = b.observed_at_unix_s - a.observed_at_unix_s;
      if (timeCmp !== 0n) return Number(timeCmp);
      return a.peer_id.localeCompare(b.peer_id);
    });
  }, [validators]);

  return (
    <>
      <Grid size={12}>
        <PageHeading>Validators</PageHeading>
      </Grid>
      <Grid size={12}>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
          The indexer periodically syncs state from random validators and records their reported consensus status on
          each visit. Each card below shows the last known status observed for that validator, along with how long ago
          the snapshot was taken.
        </Typography>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          {error ? (
            <Typography color="error">{error}</Typography>
          ) : sorted === null ? (
            <Typography color="text.secondary">Loading…</Typography>
          ) : sorted.length === 0 ? (
            <Typography color="text.secondary">
              No validators observed yet. The indexer records a snapshot the first time it syncs from each validator.
            </Typography>
          ) : (
            <Grid container spacing={2}>
              {sorted.map((v) => (
                <Grid key={v.peer_id} size={{ xs: 12, sm: 6, md: 4, lg: 3 }}>
                  <ValidatorCard validator={v} nowSec={nowSec} />
                </Grid>
              ))}
            </Grid>
          )}
        </StyledPaper>
      </Grid>
    </>
  );
}

export default Validators;
