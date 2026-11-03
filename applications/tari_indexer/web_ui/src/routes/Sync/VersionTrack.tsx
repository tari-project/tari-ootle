//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import { alpha, useTheme } from "@mui/material/styles";
import type { ShardSyncStatus } from "@tari-project/ootle-ts-bindings";
import { useId } from "react";

/** The share of the track standing for every version up to the verified one. */
const HISTORY_SHARE = 0.3;
/** The narrowest a non-empty segment is drawn, as a share of the track, so a gap of one version stays visible. */
const MIN_SEGMENT = 0.02;

/**
 * A shard's frontier as one track: the verified history, the versions staged above it awaiting a verified leaf, and
 * the versions its committee's tip has that the stream has not reached. Only the frontier is to scale; everything up
 * to the verified version is a fixed stub, since it dwarfs the rest.
 */
export default function VersionTrack({ shard }: { shard: ShardSyncStatus }) {
  const theme = useTheme();
  const hatch = useId();
  const verified = shard.verified?.state_version ?? null;
  const staged = Math.max(shard.staged?.state_version ?? -1, verified ?? -1);
  const tip = shard.tip_leaf?.state_version ?? null;

  const base = verified ?? -1;
  const end = Math.max(staged, tip ?? -1, base);
  const span = end - base;
  const frontier = 1 - HISTORY_SHARE;
  const at = (version: number) => {
    if (span <= 0) return 1;
    const share = (version - base) / span;
    return HISTORY_SHARE + frontier * share;
  };
  const widen = (from: number, to: number) => (to > from ? Math.max(to - from, MIN_SEGMENT) : 0);

  const historyEnd = verified === null ? 0 : span <= 0 ? 1 : HISTORY_SHARE;
  const stagedFrom = historyEnd;
  const stagedWidth = staged > base ? widen(stagedFrom, at(staged)) : 0;
  const unstreamedFrom = stagedFrom + stagedWidth;
  const unstreamedWidth = tip !== null && tip > staged ? Math.max(1 - unstreamedFrom, MIN_SEGMENT) : 0;

  const width = 600;
  const height = 22;
  const pct = (share: number) => share * width;
  const info = theme.palette.info.main;
  const success = theme.palette.success.main;

  return (
    <Box>
      <svg
        viewBox={`0 0 ${width} ${height}`}
        width="100%"
        height={height}
        preserveAspectRatio="none"
        role="img"
        aria-label={`Verified to ${verified === null ? "nothing" : `v${verified}`}, staged to v${staged < 0 ? "none" : staged}, tip ${tip === null ? "unknown" : `v${tip}`}`}
      >
        <defs>
          <pattern id={hatch} width={6} height={6} patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <rect width={6} height={6} fill={alpha(info, 0.18)} />
            <line x1={0} y1={0} x2={0} y2={6} stroke={info} strokeWidth={2.5} />
          </pattern>
        </defs>
        <rect x={0} y={0} width={width} height={height} rx={4} fill={alpha(theme.palette.text.primary, 0.05)} />
        {historyEnd > 0 && <rect x={0} y={0} width={pct(historyEnd)} height={height} rx={4} fill={success} />}
        {stagedWidth > 0 && (
          <rect
            x={pct(stagedFrom) + 2}
            y={0}
            width={Math.max(0, pct(stagedWidth) - 2)}
            height={height}
            fill={`url(#${hatch})`}
          />
        )}
        {unstreamedWidth > 0 && (
          <rect
            x={pct(unstreamedFrom) + 2}
            y={1}
            width={Math.max(0, pct(unstreamedWidth) - 3)}
            height={height - 2}
            rx={3}
            fill="none"
            stroke={theme.palette.text.secondary}
            strokeWidth={1.5}
            strokeDasharray="4 3"
            vectorEffect="non-scaling-stroke"
          />
        )}
      </svg>
      <Stack direction="row" flexWrap="wrap" columnGap={3} rowGap={0.5} sx={{ mt: 1 }}>
        <Key
          swatch={<Box sx={{ width: 12, height: 12, borderRadius: "3px", bgcolor: success }} />}
          label="Verified"
          value={verified === null ? "nothing yet" : `to v${verified.toLocaleString()}`}
        />
        <Key
          swatch={
            <Box
              sx={{
                width: 12,
                height: 12,
                borderRadius: "3px",
                background: `repeating-linear-gradient(45deg, ${info} 0 2px, ${alpha(info, 0.18)} 2px 5px)`,
              }}
            />
          }
          label="Staged, awaiting a leaf"
          value={staged > base ? `${(staged - base).toLocaleString()} to v${staged.toLocaleString()}` : "none"}
        />
        <Key
          swatch={
            <Box
              sx={{
                width: 12,
                height: 12,
                borderRadius: "3px",
                border: `1.5px dashed ${theme.palette.text.secondary}`,
              }}
            />
          }
          label="Not yet streamed"
          value={
            tip === null
              ? "tip unknown"
              : tip > staged
                ? `${(tip - staged).toLocaleString()} to tip v${tip.toLocaleString()}`
                : "none"
          }
        />
      </Stack>
    </Box>
  );
}

function Key({ swatch, label, value }: { swatch: React.ReactNode; label: string; value: string }) {
  return (
    <Stack direction="row" spacing={0.75} alignItems="center">
      {swatch}
      <Typography variant="caption" color="text.secondary">
        {label}
      </Typography>
      <Typography variant="caption">{value}</Typography>
    </Stack>
  );
}
