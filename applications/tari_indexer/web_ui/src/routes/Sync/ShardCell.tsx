//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import Box from "@mui/material/Box";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import { alpha, useTheme } from "@mui/material/styles";
import type { ShardSyncStatus } from "@tari-project/ootle-ts-bindings";
import { memo } from "react";
import { MapMode, STATE_LABEL, hasRecentMismatch, logScale, shardLag, stagedBacklog, stateColour } from "./model";

/** Lag at and beyond which a cell is drawn at full strength in lag mode. */
export const LAG_SATURATION = 1000;

interface Props {
  shard: ShardSyncStatus;
  mode: MapMode;
  /** The busiest shard's versions in the last minute, which activity mode saturates at. */
  maxActivity: number;
  nowSec: number;
  selected: boolean;
  onSelect: (shard: number) => void;
}

function ShardCell({ shard, mode, maxActivity, nowSec, selected, onSelect }: Props) {
  const theme = useTheme();
  const lag = shardLag(shard);
  const backlog = stagedBacklog(shard);
  const mismatch = hasRecentMismatch(shard, nowSec);
  const name = shard.shard === 0 ? "Global shard" : `Shard ${shard.shard}`;

  let background = "transparent";
  let border = "transparent";
  let fill: number | null = null;
  if (mode === "state") {
    const colour = stateColour(theme, shard.state);
    if (shard.state === "Waiting") {
      border = colour;
    } else if (shard.state === "Snapshotting") {
      background = alpha(colour, 0.2);
      fill = shard.snapshot?.fraction ?? 0;
    } else {
      background = colour;
    }
  } else if (mode === "lag") {
    if (lag === null) {
      border = theme.palette.text.disabled;
    } else {
      background = alpha(theme.palette.info.main, 0.12 + 0.88 * logScale(lag, LAG_SATURATION));
    }
  } else {
    const versions = shard.versions_last_minute;
    background =
      versions === 0
        ? alpha(theme.palette.primary.main, 0.06)
        : alpha(theme.palette.primary.main, 0.2 + 0.8 * logScale(versions, Math.max(1, maxActivity)));
  }

  const tooltip = (
    <Box sx={{ p: 0.5 }}>
      <Typography variant="subtitle2">
        {name}: {STATE_LABEL[shard.state]}
      </Typography>
      <Typography variant="caption" component="div">
        Verified {shard.verified ? `v${shard.verified.state_version}` : "nothing yet"}
        {shard.tip_leaf && `, tip v${shard.tip_leaf.state_version}`}
      </Typography>
      {lag !== null && (
        <Typography variant="caption" component="div">
          {lag === 0 ? "Verified to the tip" : `${lag.toLocaleString()} versions behind the tip`}
        </Typography>
      )}
      {backlog > 0 && (
        <Typography variant="caption" component="div">
          {backlog.toLocaleString()} staged, awaiting a verified leaf
        </Typography>
      )}
      {shard.snapshot && (
        <Typography variant="caption" component="div">
          Snapshot {Math.round(shard.snapshot.fraction * 100)}%, {shard.snapshot.substates.toLocaleString()} substates
        </Typography>
      )}
      <Typography variant="caption" component="div">
        {shard.versions_last_minute.toLocaleString()} versions promoted in the last minute
      </Typography>
      {mismatch && (
        <Typography variant="caption" component="div" color="error.light">
          A peer was blamed for this shard's history recently
        </Typography>
      )}
    </Box>
  );

  return (
    <Tooltip title={tooltip} placement="top" arrow disableInteractive>
      <Box
        component="button"
        type="button"
        onClick={() => onSelect(shard.shard)}
        aria-label={`${name}, ${STATE_LABEL[shard.state]}`}
        aria-pressed={selected}
        sx={{
          "position": "relative",
          "aspectRatio": "1",
          "width": "100%",
          "p": 0,
          "m": 0,
          "cursor": "pointer",
          "borderRadius": "3px",
          "border": `1px solid ${border}`,
          "bgcolor": background,
          "overflow": "visible",
          "outline": selected ? `2px solid ${theme.palette.text.primary}` : "none",
          "outlineOffset": "1px",
          "&:focus-visible": {
            outline: `2px solid ${theme.palette.primary.main}`,
            outlineOffset: "1px",
          },
          "&:hover": {
            filter: "brightness(1.15)",
          },
        }}
      >
        {fill !== null && (
          <Box
            sx={{
              position: "absolute",
              left: 0,
              right: 0,
              bottom: 0,
              height: `${Math.round(fill * 100)}%`,
              bgcolor: stateColour(theme, "Snapshotting"),
              borderRadius: "2px",
            }}
          />
        )}
        {mismatch && (
          <Box
            sx={{
              position: "absolute",
              top: -3,
              right: -3,
              width: 7,
              height: 7,
              borderRadius: "50%",
              bgcolor: "error.main",
              boxShadow: `0 0 0 2px ${theme.palette.background.paper}`,
            }}
          />
        )}
      </Box>
    </Tooltip>
  );
}

export default memo(ShardCell);
