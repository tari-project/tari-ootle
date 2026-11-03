//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import Box from "@mui/material/Box";
import Grid from "@mui/material/Grid";
import Stack from "@mui/material/Stack";
import ToggleButton from "@mui/material/ToggleButton";
import ToggleButtonGroup from "@mui/material/ToggleButtonGroup";
import Typography from "@mui/material/Typography";
import { alpha, useTheme } from "@mui/material/styles";
import { useQuery } from "@tanstack/react-query";
import type { GetNetworkSyncStateResponse, ShardSyncState, ShardSyncStatus } from "@tari-project/ootle-ts-bindings";
import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import PageHeading from "../../Components/PageHeading";
import { StyledPaper } from "../../Components/StyledComponents";
import { getNetworkStats } from "../../utils/api";
import GroupLane from "./GroupLane";
import { LAG_SATURATION } from "./ShardCell";
import ShardDetail from "./ShardDetail";
import Sparkline from "./Sparkline";
import {
  MapMode,
  STATE_DESCRIPTION,
  STATE_LABEL,
  STATE_ORDER,
  formatRate,
  groupContains,
  networkActivity,
  recentRate,
  stateColour,
} from "./model";

const REFRESH_INTERVAL_MS = 3000;
const MODES: MapMode[] = ["state", "lag", "activity"];

function useNowSec(): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    const tick = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(tick);
  }, []);
  return now;
}

export default function Sync() {
  const nowSec = useNowSec();
  // The mode and selected shard live in the URL, so a view of a shard can be shared
  const [params, setParams] = useSearchParams();
  const modeParam = params.get("mode") as MapMode | null;
  const mode: MapMode = modeParam && MODES.includes(modeParam) ? modeParam : "state";
  const shardParam = params.get("shard");
  const selected = shardParam !== null && /^\d+$/.test(shardParam) ? Number(shardParam) : null;
  const update = (key: string, value: string | null) =>
    setParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (value === null) next.delete(key);
        else next.set(key, value);
        return next;
      },
      { replace: true },
    );
  const { data, error, isLoading } = useQuery({
    queryKey: ["network_sync_stats"],
    queryFn: getNetworkStats,
    refetchInterval: REFRESH_INTERVAL_MS,
  });

  return (
    <>
      <Grid size={12}>
        <PageHeading>Sync</PageHeading>
      </Grid>
      <Grid size={12}>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 2, maxWidth: "75ch" }}>
          How far this indexer has verified each shard of the network. Each shard group streams from one committee
          member, and a version reaches the indexed tables only once a quorum-signed leaf covers it. Select a shard to
          see its frontier.
        </Typography>
      </Grid>
      <Grid size={12}>
        <StyledPaper>
          {error ? (
            <Typography color="error">The sync status could not be loaded: {String(error)}</Typography>
          ) : isLoading || !data ? (
            <Typography color="text.secondary">Loading…</Typography>
          ) : (
            <SyncBody
              data={data}
              nowSec={nowSec}
              mode={mode}
              onMode={(mode) => update("mode", mode === "state" ? null : mode)}
              selected={selected}
              onSelect={(shard) => update("shard", selected === shard ? null : String(shard))}
            />
          )}
        </StyledPaper>
      </Grid>
    </>
  );
}

interface BodyProps {
  data: GetNetworkSyncStateResponse;
  nowSec: number;
  mode: MapMode;
  onMode: (mode: MapMode) => void;
  selected: number | null;
  onSelect: (shard: number) => void;
}

function SyncBody({ data, nowSec, mode, onMode, selected, onSelect }: BodyProps) {
  const bucketSecs = data.activity_bucket_secs;
  const epoch = data.network_desc.epoch;
  const counts = useMemo(() => {
    const counts = new Map<ShardSyncState, number>();
    for (const shard of data.shards) counts.set(shard.state, (counts.get(shard.state) ?? 0) + 1);
    return counts;
  }, [data.shards]);
  const maxActivity = useMemo(() => Math.max(0, ...data.shards.map((s) => s.versions_last_minute)), [data.shards]);
  const activity = useMemo(() => networkActivity(data.shard_groups), [data.shard_groups]);
  const members = new Map(data.network_desc.shard_groups.map(([sg, n]) => [`${sg.start}-${sg.end_inclusive}`, n]));
  const selectedShard = data.shards.find((s) => s.shard === selected) ?? null;

  if (data.shard_groups.length === 0) {
    return (
      <Typography color="text.secondary">
        The network has no shard groups at epoch {epoch} yet. Sync starts once the first committees are registered.
      </Typography>
    );
  }

  const following = counts.get("Following") ?? 0;
  const groupShards = (start: number, end: number, first: boolean): ShardSyncStatus[] =>
    data.shards.filter((s) => (s.shard === 0 ? first : groupContains({ start, end_inclusive: end }, s.shard)));

  return (
    <Stack spacing={3}>
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: { xs: "1fr", md: "minmax(0, 1fr) 320px" },
          gap: 3,
          alignItems: "end",
        }}
      >
        <Box>
          <Typography variant="h5" component="p" sx={{ fontWeight: 600 }}>
            {following === data.shards.length
              ? `All ${data.shards.length} shards are following`
              : `${following} of ${data.shards.length} shards are following`}
          </Typography>
          <Typography variant="body2" color="text.secondary">
            Epoch {epoch}, {data.shard_groups.length} shard group{data.shard_groups.length === 1 ? "" : "s"}
          </Typography>
        </Box>
        <Box>
          <Stack direction="row" spacing={3} sx={{ mb: 0.5 }}>
            <Box>
              <Typography variant="caption" color="text.secondary">
                Network transactions per second
              </Typography>
              <Typography variant="h6" sx={{ lineHeight: 1.2 }}>
                {formatRate(recentRate(activity, bucketSecs, nowSec, 60, (b) => b.transactions))}
              </Typography>
            </Box>
            <Box>
              <Typography variant="caption" color="text.secondary">
                Versions per second
              </Typography>
              <Typography variant="h6" sx={{ lineHeight: 1.2 }}>
                {formatRate(recentRate(activity, bucketSecs, nowSec, 60, (b) => b.versions))}
              </Typography>
            </Box>
          </Stack>
          <Sparkline
            values={activity.map((b) => b.transactions / bucketSecs)}
            bucketSecs={bucketSecs}
            unit="tx/s"
            height={44}
          />
        </Box>
      </Box>

      <Stack
        direction={{ xs: "column", sm: "row" }}
        spacing={2}
        alignItems={{ sm: "center" }}
        justifyContent="space-between"
      >
        <Legend mode={mode} counts={counts} maxActivity={maxActivity} />
        <ToggleButtonGroup
          size="small"
          exclusive
          value={mode}
          onChange={(_, value: MapMode | null) => value && onMode(value)}
          aria-label="Colour shards by"
        >
          <ToggleButton value="state">State</ToggleButton>
          <ToggleButton value="lag">Lag</ToggleButton>
          <ToggleButton value="activity">Activity</ToggleButton>
        </ToggleButtonGroup>
      </Stack>

      <Box>
        {data.shard_groups.map((group, i) => (
          <GroupLane
            key={`${group.shard_group.start}-${group.shard_group.end_inclusive}`}
            group={group}
            members={members.get(`${group.shard_group.start}-${group.shard_group.end_inclusive}`) ?? null}
            shards={groupShards(group.shard_group.start, group.shard_group.end_inclusive, i === 0)}
            epoch={epoch}
            bucketSecs={bucketSecs}
            mode={mode}
            maxActivity={maxActivity}
            nowSec={nowSec}
            selected={selected}
            onSelect={onSelect}
          />
        ))}
      </Box>

      {selectedShard ? (
        <ShardDetail shard={selectedShard} nowSec={nowSec} onClose={() => onSelect(selectedShard.shard)} />
      ) : (
        <Typography variant="body2" color="text.secondary">
          Select a shard to see how far it is verified, staged and streamed.
        </Typography>
      )}
    </Stack>
  );
}

function Legend({
  mode,
  counts,
  maxActivity,
}: {
  mode: MapMode;
  counts: Map<ShardSyncState, number>;
  maxActivity: number;
}) {
  const theme = useTheme();
  if (mode === "state") {
    return (
      <Stack
        direction="row"
        flexWrap="wrap"
        columnGap={2.5}
        rowGap={1}
        component="ul"
        sx={{ m: 0, p: 0, listStyle: "none" }}
      >
        {STATE_ORDER.map((state) => {
          const colour = stateColour(theme, state);
          return (
            <Stack
              key={state}
              component="li"
              direction="row"
              spacing={0.75}
              alignItems="center"
              title={STATE_DESCRIPTION[state]}
            >
              <Box
                sx={{
                  width: 12,
                  height: 12,
                  borderRadius: "3px",
                  bgcolor: state === "Waiting" ? "transparent" : colour,
                  border: state === "Waiting" ? `1px solid ${colour}` : "none",
                }}
              />
              <Typography variant="body2">{STATE_LABEL[state]}</Typography>
              <Typography variant="body2" color="text.secondary">
                {counts.get(state) ?? 0}
              </Typography>
            </Stack>
          );
        })}
      </Stack>
    );
  }
  const colour = mode === "lag" ? theme.palette.info.main : theme.palette.primary.main;
  const label =
    mode === "lag"
      ? ["Verified to the tip", `${LAG_SATURATION.toLocaleString()}+ versions behind`]
      : ["Idle", `${maxActivity.toLocaleString()} versions in the last minute`];
  return (
    <Stack direction="row" spacing={1} alignItems="center">
      <Typography variant="caption" color="text.secondary">
        {label[0]}
      </Typography>
      <Box
        sx={{
          width: 140,
          height: 10,
          borderRadius: "3px",
          background: `linear-gradient(90deg, ${alpha(colour, 0.12)}, ${colour})`,
        }}
      />
      <Typography variant="caption" color="text.secondary">
        {label[1]}
      </Typography>
      <Typography variant="caption" color="text.secondary">
        (log scale)
      </Typography>
    </Stack>
  );
}
