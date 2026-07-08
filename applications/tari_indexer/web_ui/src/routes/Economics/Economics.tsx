//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { useRef, type ReactNode } from "react";
import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import Grid from "@mui/material/Grid";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import { alpha, useTheme } from "@mui/material/styles";
import { IoCashOutline, IoFlameOutline, IoReceiptOutline, IoWalletOutline } from "react-icons/io5";
import type { Amount } from "@tari-project/ootle-ts-bindings";
import PageHeading from "../../Components/PageHeading";
import FetchStatusCheck from "../../Components/FetchStatusCheck";
import { StyledPaper } from "../../Components/StyledComponents";
import { bigintToDecimalString } from "../../utils/helpers";
import { useNetworkEconomics } from "../../api/hooks/useNetworkEconomics";

// 1 TARI = 1_000_000 microTARI.
const TARI_DIVISIBILITY = 6;

function toBigInt(amount: Amount): bigint {
  return typeof amount === "bigint" ? amount : BigInt(amount);
}

function splitTari(amount: bigint): { whole: string; fraction: string } {
  try {
    const [whole, fraction = ""] = bigintToDecimalString(amount, TARI_DIVISIBILITY).split(".");
    return { whole, fraction: fraction.replace(/0+$/, "") };
  } catch (e) {
    console.error("Failed to format amount", amount, e);
    return { whole: "--", fraction: "" };
  }
}

// Renders the whole TARI in the surrounding font size with the fractional part and unit de-emphasized.
function TariValue({ amount }: { amount: bigint }) {
  const { whole, fraction } = splitTari(amount);
  return (
    <Box component="span">
      {whole}
      <Box component="span" sx={{ fontSize: "0.6em", color: "text.secondary", whiteSpace: "nowrap" }}>
        {fraction ? `.${fraction}` : ""}&nbsp;TARI
      </Box>
    </Box>
  );
}

function Delta({ children }: { children: ReactNode }) {
  return (
    <Typography variant="body2" sx={{ color: "success.main", fontSize: 12, lineHeight: 1.6 }}>
      ▲ {children}
    </Typography>
  );
}

function formatTari(amount: bigint): string {
  const { whole, fraction } = splitTari(amount);
  return fraction ? `${whole}.${fraction}` : whole;
}

function StatTile({
  icon,
  label,
  value,
  delta,
}: {
  icon: ReactNode;
  label: string;
  value: ReactNode;
  delta?: ReactNode;
}) {
  const theme = useTheme();
  return (
    <StyledPaper sx={{ height: "100%" }}>
      <Stack direction="row" spacing={2} alignItems="center">
        <Box
          sx={{
            width: 44,
            height: 44,
            borderRadius: "14px",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            color: theme.palette.primary.main,
            backgroundColor: alpha(theme.palette.primary.main, 0.1),
            fontSize: 22,
          }}
        >
          {icon}
        </Box>
        <Box sx={{ minWidth: 0 }}>
          <Typography variant="body2" color="textSecondary">
            {label}
          </Typography>
          <Typography variant="h4" sx={{ lineHeight: 1.4 }}>
            {value}
          </Typography>
          {delta}
        </Box>
      </Stack>
    </StyledPaper>
  );
}

// Fill = achieved burn rate, tick = target rate. The scale extends past the target so an
// over-target fill is visibly beyond the tick rather than clipped at the end of the track.
function BurnRateMeter({ achievedBps, targetBps }: { achievedBps: number | null; targetBps: number }) {
  const theme = useTheme();
  const maxBps = Math.max(targetBps * 1.25, (achievedBps ?? 0) * 1.1, 1);
  const fillPct = achievedBps === null ? 0 : Math.min((achievedBps / maxBps) * 100, 100);
  const targetPct = Math.min((targetBps / maxBps) * 100, 100);
  const tickColor = theme.palette.text.secondary;

  const achievedLabel = achievedBps === null ? "—" : `${(achievedBps / 100).toFixed(2)}%`;
  const targetLabel = `${(targetBps / 100).toFixed(2)}%`;

  return (
    <Box>
      <Tooltip title={`achieved ${achievedBps ?? "—"} bps · target ${targetBps} bps of fee volume`} arrow>
        <Box
          sx={{
            position: "relative",
            height: 10,
            borderRadius: 5,
            backgroundColor: alpha(theme.palette.primary.main, 0.12),
          }}
        >
          <Box
            sx={{
              position: "absolute",
              top: 0,
              bottom: 0,
              left: 0,
              width: `${fillPct}%`,
              borderRadius: 5,
              backgroundColor: "primary.main",
              transition: "width 0.8s ease",
            }}
          />
          <Box
            sx={{
              position: "absolute",
              top: -5,
              bottom: -5,
              left: `calc(${targetPct}% - 1px)`,
              width: 2,
              borderRadius: 1,
              backgroundColor: tickColor,
            }}
          />
        </Box>
      </Tooltip>
      <Stack direction="row" justifyContent="space-between" sx={{ mt: 1 }}>
        <Typography variant="body2" color="textSecondary">
          Achieved{" "}
          <Box component="span" sx={{ color: "text.primary", fontWeight: 600 }}>
            {achievedLabel}
          </Box>{" "}
          of fees
        </Typography>
        <Typography variant="body2" color="textSecondary">
          <Box
            component="span"
            sx={{
              display: "inline-block",
              width: 2,
              height: 10,
              borderRadius: 1,
              backgroundColor: tickColor,
              mr: 0.75,
            }}
          />
          Target {targetLabel}
        </Typography>
      </Stack>
    </Box>
  );
}

function Economics() {
  const { data, isLoading, isError, error } = useNetworkEconomics();

  return (
    <>
      <Grid size={12}>
        <PageHeading>Network Economics</PageHeading>
      </Grid>
      <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message ?? ""}>
        {data ? <EconomicsContent data={data} /> : null}
      </FetchStatusCheck>
    </>
  );
}

function EconomicsContent({ data }: { data: NonNullable<ReturnType<typeof useNetworkEconomics>["data"]> }) {
  const theme = useTheme();
  const feeVolume = toBigInt(data.fee_volume);
  const receiptBurn = toBigInt(data.receipt_exhaust_burned);
  const txCount = Number(data.transaction_receipt_count);

  // Totals are cumulative, so the first response is the session baseline; a decrease means the
  // indexer was reset, in which case the baseline restarts rather than showing a negative delta.
  const baseline = useRef<{ fee: bigint; burn: bigint; tx: number } | null>(null);
  if (
    baseline.current === null ||
    feeVolume < baseline.current.fee ||
    receiptBurn < baseline.current.burn ||
    txCount < baseline.current.tx
  ) {
    baseline.current = { fee: feeVolume, burn: receiptBurn, tx: txCount };
  }
  const feeDelta = feeVolume - baseline.current.fee;
  const burnDelta = receiptBurn - baseline.current.burn;
  const txDelta = txCount - baseline.current.tx;

  // Realized rate is computed from the same receipt source on both sides so it recovers the true rate `r`
  // independent of the header-sourced burn total.
  const achievedBps = feeVolume > 0n ? Number((receiptBurn * 10000n) / feeVolume) : null;

  return (
    <>
      <Grid size={12}>
        <StyledPaper
          sx={{
            backgroundImage: `linear-gradient(120deg, ${alpha(theme.palette.primary.main, 0.08)} 0%, transparent 60%)`,
          }}
        >
          <Stack direction="row" justifyContent="space-between" alignItems="flex-start" spacing={2}>
            <Typography variant="body2" color="textSecondary" gutterBottom>
              Total TARI supply
            </Typography>
            <Chip size="small" variant="outlined" label={`Epoch ${data.current_epoch}`} />
          </Stack>
          <Typography
            variant="h1"
            component="div"
            sx={{ fontSize: { xs: "2.2rem", sm: "3rem" }, lineHeight: 1.25, mt: 0.5 }}
          >
            <TariValue amount={toBigInt(data.total_supply)} />
          </Typography>
          <Typography variant="body2" color="textSecondary" sx={{ mt: 1 }}>
            Total claimed less exhaust burned
          </Typography>
        </StyledPaper>
      </Grid>

      <Grid size={{ xs: 12, sm: 6, md: 4 }}>
        <StatTile
          icon={<IoWalletOutline />}
          label="Total claimed"
          value={<TariValue amount={toBigInt(data.total_claimed)} />}
        />
      </Grid>
      <Grid size={{ xs: 12, sm: 6, md: 4 }}>
        <StatTile
          icon={<IoCashOutline />}
          label="Fee volume"
          value={<TariValue amount={feeVolume} />}
          delta={feeDelta > 0n && <Delta>{formatTari(feeDelta)} TARI</Delta>}
        />
      </Grid>
      <Grid size={{ xs: 12, sm: 6, md: 4 }}>
        <StatTile
          icon={<IoReceiptOutline />}
          label="Transactions"
          value={txCount.toLocaleString()}
          delta={txDelta > 0 && <Delta>{txDelta.toLocaleString()}</Delta>}
        />
      </Grid>

      <Grid size={12}>
        <StyledPaper>
          <Stack
            direction={{ xs: "column", md: "row" }}
            spacing={{ xs: 3, md: 6 }}
            alignItems={{ xs: "stretch", md: "center" }}
          >
            <Stack direction="row" spacing={2} alignItems="center" sx={{ flexShrink: 0 }}>
              <Box
                sx={{
                  width: 44,
                  height: 44,
                  borderRadius: "14px",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                  color: theme.palette.primary.main,
                  backgroundColor: alpha(theme.palette.primary.main, 0.1),
                  fontSize: 22,
                }}
              >
                <IoFlameOutline />
              </Box>
              <Box>
                <Typography variant="body2" color="textSecondary">
                  Exhaust burn
                </Typography>
                <Typography variant="h4" sx={{ lineHeight: 1.4 }}>
                  <TariValue amount={receiptBurn} />
                </Typography>
                {burnDelta > 0n && <Delta>{formatTari(burnDelta)} TARI</Delta>}
              </Box>
            </Stack>
            <Box sx={{ flexGrow: 1 }}>
              <BurnRateMeter achievedBps={achievedBps} targetBps={data.target_burn_rate_bps} />
            </Box>
          </Stack>
        </StyledPaper>
      </Grid>

      <Grid size={12}>
        <Typography variant="body2" color="textSecondary" align="center" sx={{ fontSize: 12 }}>
          Cumulative network totals since genesis · refreshes every 30 seconds
        </Typography>
      </Grid>
    </>
  );
}

export default Economics;
