//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import Grid from "@mui/material/Grid";
import Typography from "@mui/material/Typography";
import type { Amount } from "@tari-project/ootle-ts-bindings";
import PageHeading from "../../Components/PageHeading";
import FetchStatusCheck from "../../Components/FetchStatusCheck";
import { StyledPaper } from "../../Components/StyledComponents";
import { bigintToDecimalString } from "../../utils/helpers";
import { useNetworkEconomics } from "../../api/hooks/useNetworkEconomics";

// 1 TARI = 1_000_000 microTARI.
const TARI_DIVISIBILITY = 6;

function toTari(amount: Amount): string {
  try {
    return `${bigintToDecimalString(amount, TARI_DIVISIBILITY)} TARI`;
  } catch (e) {
    console.error("Failed to format amount", amount, e);
    return "-- TARI";
  }
}

function toBigInt(amount: Amount): bigint {
  return typeof amount === "bigint" ? amount : BigInt(amount);
}

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <StyledPaper sx={{ height: "100%" }}>
      <Typography variant="body2" color="textSecondary" gutterBottom>
        {label}
      </Typography>
      <Typography variant="h5">{value}</Typography>
    </StyledPaper>
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
  const feeVolume = toBigInt(data.fee_volume);
  const receiptBurn = toBigInt(data.receipt_exhaust_burned);

  // Realized rate is computed from the same receipt source on both sides so it recovers the true rate `r`
  // independent of the header-sourced burn total.
  const achievedBps = feeVolume > 0n ? Number((receiptBurn * 10000n) / feeVolume) : null;
  const achievedPct = achievedBps === null ? null : achievedBps / 100;
  const targetPct = data.target_burn_rate_bps / 100;

  return (
    <>
      <Grid size={12}>
        <StyledPaper>
          <Typography variant="body2" color="textSecondary" gutterBottom>
            Total TARI Supply · epoch {data.current_epoch}
          </Typography>
          <Typography variant="h3">{toTari(data.total_supply)}</Typography>
        </StyledPaper>
      </Grid>

      <Grid size={{ xs: 12, sm: 6, md: 4 }}>
        <StatCard label="Total Claimed" value={toTari(data.total_claimed)} />
      </Grid>
      <Grid size={{ xs: 12, sm: 6, md: 4 }}>
        <StatCard label="Fee Volume" value={toTari(data.fee_volume)} />
      </Grid>
      <Grid size={{ xs: 12, sm: 6, md: 4 }}>
        <StatCard label="Transaction Count" value={data.transaction_receipt_count.toLocaleString()} />
      </Grid>

      <Grid size={12}>
        <StyledPaper>
          <Typography variant="body2" color="textSecondary" gutterBottom>
            Exhaust Burn
          </Typography>
          <Typography variant="h4">{toTari(data.receipt_exhaust_burned)} burnt</Typography>
          <Typography variant="body1" color="textSecondary" sx={{ mt: 1 }}>
            achieved {achievedPct === null ? "—" : `${achievedPct.toFixed(2)}%`} · target {targetPct.toFixed(2)}%
          </Typography>
        </StyledPaper>
      </Grid>
    </>
  );
}

export default Economics;
