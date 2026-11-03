//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import type { Epoch, ShardGroupSyncStatus, ShardSyncStatus } from "@tari-project/ootle-ts-bindings";
import CopyToClipboard from "../../Components/CopyToClipboard";
import { shortenString } from "../VN/Components/helpers";
import ShardCell from "./ShardCell";
import Sparkline from "./Sparkline";
import {
  MapMode,
  formatAge,
  formatDuration,
  formatRate,
  formatShardGroup,
  phaseColour,
  phaseLabel,
  recentRate,
} from "./model";

interface Props {
  group: ShardGroupSyncStatus;
  members: number | null;
  /** The group's shards, ascending, with the global shard first when the group syncs it. */
  shards: ShardSyncStatus[];
  epoch: Epoch;
  bucketSecs: number;
  mode: MapMode;
  maxActivity: number;
  nowSec: number;
  selected: number | null;
  onSelect: (shard: number) => void;
}

const mono = { fontFamily: "'Courier New', Courier, monospace" };

export default function GroupLane({
  group,
  members,
  shards,
  epoch,
  bucketSecs,
  mode,
  maxActivity,
  nowSec,
  selected,
  onSelect,
}: Props) {
  const phase = group.phase;
  const blamedThisEpoch = group.blames.filter((blame) => blame.epoch === epoch);
  const excluded = group.excluded_peers;
  const txRate = recentRate(group.activity, bucketSecs, nowSec, 60, (b) => b.transactions);
  const versionRate = recentRate(group.activity, bucketSecs, nowSec, 60, (b) => b.versions);
  const global = shards.find((shard) => shard.shard === 0);
  const preshards = shards.filter((shard) => shard.shard !== 0);

  const cell = (shard: ShardSyncStatus) => (
    <ShardCell
      key={shard.shard}
      shard={shard}
      mode={mode}
      maxActivity={maxActivity}
      nowSec={nowSec}
      selected={selected === shard.shard}
      onSelect={onSelect}
    />
  );

  return (
    <Box
      component="section"
      aria-label={formatShardGroup(group.shard_group)}
      sx={{
        "display": "grid",
        "gridTemplateColumns": { xs: "1fr", md: "240px minmax(0, 1fr)", lg: "240px minmax(0, 1fr) 200px" },
        "gap": { xs: 2, md: 3 },
        "py": 2.5,
        "borderTop": 1,
        "borderColor": "divider",
        "&:first-of-type": { borderTop: 0 },
      }}
    >
      <Stack spacing={0.75} sx={{ minWidth: 0 }}>
        <Typography variant="subtitle1" sx={{ fontWeight: 600, lineHeight: 1.2 }}>
          {formatShardGroup(group.shard_group)}
        </Typography>
        {members !== null && (
          <Typography variant="caption" color="text.secondary">
            {members} committee member{members === 1 ? "" : "s"}
          </Typography>
        )}
        <Stack direction="row" spacing={1} alignItems="center">
          <Chip label={phaseLabel(phase)} color={phaseColour(phase)} size="small" sx={{ fontWeight: 600 }} />
          <Typography variant="caption" color="text.secondary">
            for {formatDuration(nowSec - group.phase_since_unix_s)}
          </Typography>
        </Stack>
        {phase.kind === "Paused" && (
          <Typography variant="caption" color="text.secondary">
            {phase.reason}, retrying in {formatDuration(phase.until_unix_s - nowSec)}
          </Typography>
        )}
        {phase.kind === "Halted" && (
          <Typography variant="caption" color="error">
            Every member's history of shard {phase.shard} mismatched
            {phase.from_version !== null ? ` above v${phase.from_version}` : ""}. Stopped until the epoch moves.
          </Typography>
        )}
        {group.stream_peer && (
          <Stack direction="row" spacing={0.5} alignItems="center">
            <Typography variant="caption" color="text.secondary">
              {phase.kind === "Streaming" || phase.kind === "Snapshotting" ? "From" : "Last from"}
            </Typography>
            <Typography variant="caption" sx={mono}>
              {shortenString(group.stream_peer, 6, 6)}
            </Typography>
            <CopyToClipboard copy={group.stream_peer} />
          </Stack>
        )}
        <Typography variant="caption" color="text.secondary">
          {group.tip
            ? `Tip at epoch ${group.tip.epoch}, height ${group.tip.height.toLocaleString()}, probed ${formatAge(nowSec - group.tip.observed_at_unix_s)}`
            : "No tip probed yet"}
        </Typography>
        <Typography variant="caption" color="text.secondary">
          {group.last_checkpoint
            ? `Checkpoint of epoch ${group.last_checkpoint.epoch} stored ${formatAge(nowSec - group.last_checkpoint.stored_at_unix_s)}`
            : "No checkpoint stored since start"}
        </Typography>
        {excluded.length > 0 && (
          <Tooltip
            arrow
            title={
              <Stack spacing={1} sx={{ p: 0.5 }}>
                {blamedThisEpoch.map((blame, i) => (
                  <Typography key={i} variant="caption" sx={{ wordBreak: "break-word" }}>
                    {formatAge(nowSec - blame.at_unix_s)}: {blame.details}
                  </Typography>
                ))}
              </Stack>
            }
          >
            <Typography variant="caption" color="error" sx={{ cursor: "help", textDecoration: "underline dotted" }}>
              {excluded.length} peer{excluded.length === 1 ? "" : "s"} excluded this epoch
            </Typography>
          </Tooltip>
        )}
      </Stack>

      <Box sx={{ minWidth: 0 }}>
        {global && (
          <Stack direction="row" spacing={1} alignItems="center" sx={{ mb: 1 }}>
            <Box sx={{ width: { xs: 16, md: 22 }, flexShrink: 0 }}>{cell(global)}</Box>
            <Typography variant="caption" color="text.secondary">
              Global shard
            </Typography>
          </Stack>
        )}
        <Box
          role="group"
          aria-label={`${formatShardGroup(group.shard_group)} shard map`}
          sx={{
            display: "grid",
            gridTemplateColumns: { xs: "repeat(16, minmax(0, 1fr))", md: "repeat(32, minmax(0, 22px))" },
            gap: "3px",
          }}
        >
          {preshards.map(cell)}
        </Box>
      </Box>

      <Stack
        direction={{ xs: "column", sm: "row", lg: "column" }}
        spacing={{ xs: 1.5, sm: 3, lg: 1.5 }}
        sx={{
          "minWidth": 0,
          "gridColumn": { md: "1 / -1", lg: "auto" },
          "& > *": { flex: { sm: 1, lg: "none" }, minWidth: 0 },
        }}
      >
        <Box>
          <Typography variant="caption" color="text.secondary">
            Transactions per second
          </Typography>
          <Typography variant="h6" sx={{ lineHeight: 1.2 }}>
            {formatRate(txRate)}
          </Typography>
          <Sparkline
            values={group.activity.map((b) => b.transactions / bucketSecs)}
            bucketSecs={bucketSecs}
            unit="tx/s"
          />
        </Box>
        <Box>
          <Typography variant="caption" color="text.secondary">
            Versions per second
          </Typography>
          <Typography variant="h6" sx={{ lineHeight: 1.2 }}>
            {formatRate(versionRate)}
          </Typography>
          <Sparkline
            values={group.activity.map((b) => b.versions / bucketSecs)}
            bucketSecs={bucketSecs}
            unit="versions/s"
          />
        </Box>
      </Stack>
    </Box>
  );
}
