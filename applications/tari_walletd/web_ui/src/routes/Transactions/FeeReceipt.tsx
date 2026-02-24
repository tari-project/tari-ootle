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

import { TableContainer, TableRow, Box, Typography, Chip } from "@mui/material";
import { DataTableCell } from "@components/StyledComponents";
import { formatCurrency } from "@/utils/helpers";
import { XTR_CURRENCY } from "@utils/constants";
import { FeeReceipt as FeeReceiptProps } from "@tari-project/ootle-ts-bindings";

function unsignedSaturatingSub(a: bigint): bigint {
  return a < BigInt(0) ? BigInt(0) : a;
}
export default function FeeReceipt({ data }: { data: FeeReceiptProps }) {
  if (!data) {
    return (
      <Box sx={{ p: 3, textAlign: "center" }}>
        <Typography variant="body2" color="text.secondary">
          No fee receipt available
        </Typography>
      </Box>
    );
  }

  const totalCost = Object.entries(data.cost_breakdown?.breakdown || {}).reduce(
    (sum, [_, value]) => BigInt(sum) + BigInt(value),
    BigInt(0),
  );

  const feeItems = [
    {
      label: "Total Fees Paid",
      value: formatCurrency(data.total_fee_payment, XTR_CURRENCY.SYMBOL),
      color: "primary" as const,
    },
    {
      label: "Total Fees Charged",
      value: formatCurrency(data.total_fees_paid, XTR_CURRENCY.SYMBOL),
      color: "success" as const,
    },
    {
      label: "Total Fees Required",
      value: formatCurrency(totalCost, XTR_CURRENCY.SYMBOL),
      color: "success" as const,
    },
    {
      label: "Fees Refunded",
      value: formatCurrency(unsignedSaturatingSub(BigInt(data.total_fee_payment) - totalCost), XTR_CURRENCY.SYMBOL),
      color: "success" as const,
    },
    {
      label: "Fees Overcharge",
      value: formatCurrency(data.total_fee_overcharge, XTR_CURRENCY.SYMBOL),
      color: "success" as const,
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
      {feeItems.map((item, index) => (
        <TableRow key={index}>
          <DataTableCell>
            <Typography variant="body2">{item.label}</Typography>
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
                <Chip key={key} label={`${key}: ${value}`} size="small" color="default" variant="outlined" />
              ))}
            </Box>
          </DataTableCell>
        </TableRow>
      ))}
    </TableContainer>
  );
}
