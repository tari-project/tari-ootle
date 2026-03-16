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

import {
  Box,
  Chip,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import { ReactNode } from "react";
import { DataTableCell } from "../../../Components/StyledComponents";

function ResultRowData({ result, index }: { result: any; index: number }) {
  const returnTypeLabel =
    typeof result.return_type === "string" ? result.return_type : result.return_type?.Other?.name || "Unknown";

  const renderValue = () => {
    if (!result.indexed?.value) return null;
    if (typeof result.indexed.value === "string") return result.indexed.value;
    if (result.indexed.value.Tag) {
      return `Tag ${result.indexed.value.Tag[0]}: ${JSON.stringify(result.indexed.value.Tag[1])}`;
    }
    return JSON.stringify(result.indexed.value);
  };

  const renderIndexedData = () => {
    if (!result.indexed?.indexed) return null;
    const indexed = result.indexed.indexed;
    const sections: ReactNode[] = [];
    const fieldConfig = [
      { key: "bucket_ids", label: "Bucket IDs", color: "primary" },
      { key: "component_addresses", label: "Component Addresses", color: "info" },
      { key: "component_address_allocations", label: "Component Address Allocations", color: "info" },
      { key: "resource_addresses", label: "Resource Addresses", color: "success" },
      { key: "resource_address_allocations", label: "Resource Address Allocations", color: "success" },
      { key: "vault_ids", label: "Vault IDs", color: "warning" },
      { key: "non_fungible_addresses", label: "NFT Addresses", color: "secondary" },
      { key: "proof_ids", label: "Proof IDs", color: "error" },
      { key: "metadata", label: "Metadata", color: "default" },
      { key: "published_template_addresses", label: "Published Template Addresses", color: "primary" },
      { key: "transaction_receipt_addresses", label: "Transaction Receipt Addresses", color: "info" },
      { key: "unclaimed_confidential_output_address", label: "Unclaimed Confidential Output Address", color: "warning" },
      { key: "utxos", label: "UTXOs", color: "success" },
      { key: "validator_node_fee_pools", label: "Validator Node Fee Pools", color: "secondary" },
    ];
    fieldConfig.forEach(({ key, label, color }) => {
      if (indexed[key]?.length > 0) {
        sections.push(
          <Stack key={key} direction="row" spacing={1} alignItems="center" sx={{ mb: 1, flexWrap: "wrap" }}>
            <Typography variant="body2" color="text.secondary" sx={{ mb: 0.5 }}>
              {label}:
            </Typography>
            <Box sx={{ display: "flex", gap: 0.5, flexWrap: "wrap" }}>
              {indexed[key].map((item: any, idx: number) => (
                <Chip key={idx} label={item} size="small" color={color as any} variant="outlined" />
              ))}
            </Box>
          </Stack>,
        );
      }
    });
    return sections.length > 0 ? sections : null;
  };

  return (
    <TableRow key={index}>
      <DataTableCell width={150} sx={{ borderTop: 1, borderTopColor: "divider" }}>
        <Typography variant="body2">Result #{index + 1}</Typography>
      </DataTableCell>
      <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider" }}>
        <Chip label={returnTypeLabel} size="small" color="secondary" variant="outlined" />
      </DataTableCell>
      <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider" }}>
        {renderValue() || "--"}
      </DataTableCell>
      <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider" }}>
        {renderIndexedData() && <Box sx={{ mb: 2 }}>{renderIndexedData()}</Box>}
      </DataTableCell>
    </TableRow>
  );
}

export default function ExecutionResults({ data }: { data: any[] }) {
  if (!data || data.length === 0) {
    return (
      <Box sx={{ p: 3, textAlign: "center" }}>
        <Typography variant="body2" color="text.secondary">
          No execution results available
        </Typography>
      </Box>
    );
  }
  return (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <TableCell>Result</TableCell>
            <TableCell>Return Type</TableCell>
            <TableCell>Value</TableCell>
            <TableCell>Indexed Data</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {data.map((result, index) => (
            <ResultRowData key={index} result={result} index={index} />
          ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
