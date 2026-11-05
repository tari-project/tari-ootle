//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import LinearProgress from "@mui/material/LinearProgress";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type { ShardSyncStatus } from "@tari-project/ootle-ts-bindings";
import VersionTrack from "./VersionTrack";
import { STATE_DESCRIPTION, STATE_LABEL, formatAge, formatDuration, shardLag, stagedBacklog } from "./model";

interface Props {
  shard: ShardSyncStatus;
  nowSec: number;
  onClose: () => void;
}

export default function ShardDetail({ shard, nowSec, onClose }: Props) {
  const lag = shardLag(shard);
  const backlog = stagedBacklog(shard);
  const facts: [string, string][] = [
    [
      "Verified",
      shard.verified
        ? `v${shard.verified.state_version.toLocaleString()}, committed at epoch ${shard.verified.epoch}`
        : "Nothing yet",
    ],
    [
      "Staged",
      shard.staged
        ? `v${shard.staged.state_version.toLocaleString()}, ${backlog.toLocaleString()} awaiting a leaf`
        : "Nothing yet",
    ],
    [
      "Tip leaf",
      shard.tip_leaf
        ? `v${shard.tip_leaf.state_version.toLocaleString()}, probed ${formatAge(nowSec - shard.tip_leaf.observed_at_unix_s)}`
        : "Not probed yet",
    ],
    ["Behind the tip", lag === null ? "Unknown" : `${lag.toLocaleString()} version${lag === 1 ? "" : "s"}`],
    [
      "Level with committee",
      shard.level
        ? `At v${shard.level.state_version.toLocaleString()}, confirmed ${formatAge(shard.level.confirmed_secs_ago)}`
        : "Not since start",
    ],
    [
      "Last promotion",
      shard.last_promoted_at_unix_s === null
        ? "None since start"
        : `${formatAge(nowSec - shard.last_promoted_at_unix_s)}, ${shard.versions_last_minute.toLocaleString()} versions in the last minute`,
    ],
  ];
  if (shard.snapshot) {
    facts.push([
      "Snapshot",
      `At v${shard.snapshot.state_version.toLocaleString()}, ${Math.round(shard.snapshot.fraction * 100)}% of the key space, ${shard.snapshot.substates.toLocaleString()} substates in ${formatDuration(nowSec - shard.snapshot.started_at_unix_s)}`,
    ]);
  }

  return (
    <Box
      component="section"
      aria-label={`Shard ${shard.shard} detail`}
      sx={{ borderTop: 1, borderColor: "divider", pt: 3 }}
    >
      <Stack direction="row" justifyContent="space-between" alignItems="baseline" spacing={2} sx={{ mb: 2 }}>
        <Box>
          <Typography variant="h6" component="h2">
            {shard.shard === 0 ? "Global shard" : `Shard ${shard.shard}`}: {STATE_LABEL[shard.state]}
          </Typography>
          <Typography variant="body2" color="text.secondary">
            {STATE_DESCRIPTION[shard.state]}
          </Typography>
        </Box>
        <Button size="small" onClick={onClose}>
          Close
        </Button>
      </Stack>
      {shard.snapshot ? (
        <Box>
          <LinearProgress
            variant="determinate"
            value={shard.snapshot.fraction * 100}
            aria-label="Snapshot progress"
            sx={{ height: 22, borderRadius: "4px" }}
          />
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 1 }}>
            Snapshot at v{shard.snapshot.state_version.toLocaleString()}: {Math.round(shard.snapshot.fraction * 100)}%
            of the shard's leaf key space received
          </Typography>
        </Box>
      ) : (
        <VersionTrack shard={shard} />
      )}
      <Box
        component="dl"
        sx={{
          "display": "grid",
          "gridTemplateColumns": { xs: "1fr", sm: "max-content 1fr" },
          "columnGap": 3,
          "rowGap": { xs: 0.25, sm: 1 },
          "mt": 3,
          "mb": 0,
          "& dt": { color: "text.secondary", mt: { xs: 1, sm: 0 } },
          "& dd": { m: 0 },
        }}
      >
        {facts.map(([term, value]) => (
          <Box key={term} sx={{ display: "contents" }}>
            <Typography component="dt" variant="body2">
              {term}
            </Typography>
            <Typography component="dd" variant="body2">
              {value}
            </Typography>
          </Box>
        ))}
      </Box>
      {shard.last_mismatch && (
        <Box sx={{ mt: 3 }}>
          <Typography variant="body2" color="error" sx={{ mb: 0.5 }}>
            Last blame for this shard's history, {formatAge(nowSec - shard.last_mismatch.at_unix_s)}
          </Typography>
          <Typography
            variant="body2"
            sx={{
              fontFamily: "'Courier New', Courier, monospace",
              fontSize: 13,
              wordBreak: "break-word",
              bgcolor: "divider",
              borderRadius: 1,
              p: 1.5,
            }}
          >
            {shard.last_mismatch.details}
          </Typography>
        </Box>
      )}
    </Box>
  );
}
