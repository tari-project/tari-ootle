//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { alpha, useTheme } from "@mui/material/styles";
import { useState } from "react";
import { formatRate } from "./model";

interface Props {
  /** Per-second rate in each bucket, oldest first. */
  values: number[];
  bucketSecs: number;
  unit: string;
  height?: number;
}

/**
 * A rate over the last ten minutes as a line on its own zero-based scale, with a crosshair that reads out the bucket
 * under the pointer.
 */
export default function Sparkline({ values, bucketSecs, unit, height = 36 }: Props) {
  const theme = useTheme();
  const [hover, setHover] = useState<number | null>(null);
  const width = 200;
  const max = Math.max(...values, 0);
  const n = values.length;
  const x = (i: number) => (n <= 1 ? width : (i / (n - 1)) * width);
  const y = (v: number) => (max === 0 ? height - 1 : height - 1 - (v / max) * (height - 4));
  const line = values.map((v, i) => `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join("");
  const area = `${line}L${width},${height}L0,${height}Z`;
  const colour = theme.palette.primary.main;

  const onMove = (e: React.PointerEvent<SVGSVGElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const i = Math.round(((e.clientX - rect.left) / rect.width) * (n - 1));
    setHover(Math.max(0, Math.min(n - 1, i)));
  };

  const hoverAgo = hover === null ? 0 : (n - 1 - hover) * bucketSecs;

  return (
    <Box sx={{ position: "relative", width: "100%" }}>
      <svg
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        width="100%"
        height={height}
        role="img"
        aria-label={`${unit} over the last ${Math.round((n * bucketSecs) / 60)} minutes, peaking at ${formatRate(max)}`}
        onPointerMove={onMove}
        onPointerLeave={() => setHover(null)}
        style={{ display: "block", overflow: "visible", cursor: "crosshair" }}
      >
        <path d={area} fill={alpha(colour, 0.12)} />
        <path d={line} fill="none" stroke={colour} strokeWidth={2} vectorEffect="non-scaling-stroke" />
        {hover !== null && (
          <>
            <line
              x1={x(hover)}
              x2={x(hover)}
              y1={0}
              y2={height}
              stroke={theme.palette.text.secondary}
              strokeWidth={1}
              vectorEffect="non-scaling-stroke"
            />
            <circle
              cx={x(hover)}
              cy={y(values[hover])}
              r={3}
              fill={colour}
              stroke={theme.palette.background.paper}
              strokeWidth={2}
              vectorEffect="non-scaling-stroke"
            />
          </>
        )}
      </svg>
      {hover !== null && (
        <Typography
          variant="caption"
          sx={{
            position: "absolute",
            bottom: height + 4,
            left: `${(x(hover) / width) * 100}%`,
            transform: "translateX(-50%)",
            whiteSpace: "nowrap",
            bgcolor: "background.paper",
            border: 1,
            borderColor: "divider",
            borderRadius: 1,
            px: 0.75,
            py: 0.25,
            pointerEvents: "none",
            zIndex: 1,
          }}
          aria-hidden
        >
          {formatRate(values[hover])} {unit}, {hoverAgo === 0 ? "now" : `${Math.round(hoverAgo / 60) || "<1"}m ago`}
        </Typography>
      )}
    </Box>
  );
}
