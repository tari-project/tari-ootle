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

import { TableContainer, Table, TableHead, TableRow, TableCell, TableBody, Box, Typography, Chip } from "@mui/material";
import { DataTableCell } from "@components/StyledComponents";

export default function FeeReceipt({ data }: { data: any }) {
  if (!data) {
    return (
      <Box sx={{ p: 3, textAlign: "center" }}>
        <Typography variant="body2" color="text.secondary">
          No fee receipt available
        </Typography>
      </Box>
    );
  }

  const feeItems = [
    { label: "Total Fee Payment", value: data.total_fee_payment, color: "primary" as const },
    { label: "Total Fees Paid", value: data.total_fees_paid, color: "success" as const },
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
        <TableHead>
          <TableRow>
            <TableCell>Fee Type</TableCell>
            <TableCell>Amount</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
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
        </TableBody>
      </Table>
    </TableContainer>
  );
}
