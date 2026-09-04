//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import { formatCurrency } from "@/utils/helpers";
import { DataTableCell } from "@components/StyledComponents";
import HelpOutlineIcon from "@mui/icons-material/HelpOutline";
import { Box, Chip, TableContainer, TableRow, Tooltip, Typography } from "@mui/material";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import { FeeReceipt as FeeReceiptProps } from "@tari-project/ootle-ts-bindings";
import { XTR_CURRENCY } from "@utils/currency";

const FEE_SOURCE_LABELS: Record<string, string> = {
  Initial: "Initial",
  RuntimeCall: "Runtime calls",
  Storage: "Storage",
  TransactionWeight: "Transaction weight",
  SignatureVerification: "Signature verification",
  TemplateLoad: "Template load",
  SubstateCreate: "Substate creation",
  WasmExecution: "WASM execution",
  TemplatePublish: "Template publish",
  ExhaustBurn: "Exhaust burn",
};

function unsignedSaturatingSub(a: bigint): bigint {
  return a < BigInt(0) ? BigInt(0) : a;
}
export default function FeeReceipt({ data, finalFee }: { data: FeeReceiptProps; finalFee?: number | null }) {
  if (!data) {
    return (
      <Box sx={{ p: 3, textAlign: "center" }}>
        <Typography variant="body2" color="text.secondary">
          No fee receipt available
        </Typography>
      </Box>
    );
  }

  const totalFeesCharged = Object.entries(data.cost_breakdown?.breakdown || {}).reduce(
    (sum, [_, value]) => BigInt(sum) + BigInt(value),
    BigInt(0),
  );

  // If final_fee is available, use it as the actual fee paid to compute accurate overcharge/refund
  const totalFeePayment = finalFee != null ? BigInt(finalFee) : BigInt(data.total_fee_payment);
  const totalRefunded = unsignedSaturatingSub(totalFeePayment - totalFeesCharged - BigInt(data.total_fee_overcharge));
  const overcharge = unsignedSaturatingSub(totalFeePayment - totalFeesCharged - totalRefunded);

  // The burn is a share of what was paid, so the rate shown is burn / paid to one decimal place
  const exhaustBurn = BigInt(data.exhaust_burn ?? 0);
  const totalFeesPaid = BigInt(data.total_fees_paid);
  const burnPercent =
    exhaustBurn > BigInt(0) && totalFeesPaid > BigInt(0) ? Number((exhaustBurn * BigInt(1000)) / totalFeesPaid) / 10 : null;

  const feeItems: { label: string; value: string; color: "primary" | "success"; help?: string }[] = [
    {
      label: "Total Fees Paid",
      value: formatCurrency(totalFeePayment, XTR_CURRENCY),
      color: "primary",
    },
    {
      label: "Total Fees Charged",
      value: formatCurrency(totalFeesCharged, XTR_CURRENCY),
      color: "success",
    },
    {
      label: "Exhaust Burn",
      value: `${formatCurrency(exhaustBurn, XTR_CURRENCY)}${burnPercent !== null ? ` (~${burnPercent}%)` : ""}`,
      color: "success",
      help:
        "The share of the fees paid that is permanently burned, reducing the total supply. " +
        "Validators receive the remainder.",
    },
    {
      label: "Fees Refunded",
      value: formatCurrency(totalRefunded, XTR_CURRENCY),
      color: "success",
    },
    {
      label: "Fees Overcharge",
      value: formatCurrency(overcharge, XTR_CURRENCY),
      color: "success",
      help:
        "An overcharge occurs when paying more fees than required using stealth transfers. To preserve privacy, " +
        "there is no vault to refund excess fees, therefore the fees are given to validators in their entirety.",
    },
  ];

  const costBreakdownItems = data.cost_breakdown?.breakdown
    ? [
        {
          label: "Cost Breakdown",
          breakdown: data.cost_breakdown.breakdown,
        },
      ]
    : [];

  return (
    <TableContainer>
      <Table>
        <TableBody>
          {feeItems.map((item, index) => (
            <TableRow key={index}>
              <DataTableCell>
                <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
                  <Typography variant="body2">{item.label}</Typography>
                  {item.help && (
                    <Tooltip title={item.help} arrow>
                      <HelpOutlineIcon sx={{ fontSize: 16, color: "text.secondary" }} />
                    </Tooltip>
                  )}
                </Box>
              </DataTableCell>
              <DataTableCell>{item.value}</DataTableCell>
            </TableRow>
          ))}
          {costBreakdownItems.map((item, index) => (
            <TableRow key={`breakdown-${index}`}>
              <DataTableCell>
                <Typography variant="body2">{item.label}</Typography>
              </DataTableCell>
              <DataTableCell>
                <Box sx={{ display: "flex", flexWrap: "wrap", gap: 1 }}>
                  {Object.entries(item.breakdown).map(([key, value]) => (
                    <Chip
                      key={key}
                      label={`${FEE_SOURCE_LABELS[key] ?? key}: ${value}`}
                      size="small"
                      color="default"
                      variant="outlined"
                    />
                  ))}
                </Box>
              </DataTableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
