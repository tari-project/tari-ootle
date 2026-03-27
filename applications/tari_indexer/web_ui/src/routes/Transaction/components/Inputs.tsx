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

import { Box, Chip, Stack, Table, TableBody, TableCell, TableContainer, TableHead, TableRow, Typography } from "@mui/material";
import { SubstateRequirement, substateIdToString } from "@tari-project/ootle-ts-bindings";
import CopyToClipboard from "../../../Components/CopyToClipboard";
import { DataTableCell } from "../../../Components/StyledComponents";

function getSubstateType(substateId: string): string {
  if (substateId.startsWith("component_")) return "Component";
  if (substateId.startsWith("vault_")) return "Vault";
  if (substateId.startsWith("resource_")) return "Resource";
  if (substateId.startsWith("nft_")) return "NFT";
  if (substateId.startsWith("commitment_")) return "Commitment";
  if (substateId.startsWith("txreceipt_")) return "Transaction Receipt";
  if (substateId.startsWith("template_")) return "Template";
  if (substateId.startsWith("utxo_")) return "Utxo";
  if (substateId.startsWith("vnfp_")) return "VnFeePool";
  return "Unknown";
}

function getTypeColor(type: string): "primary" | "secondary" | "success" | "warning" | "info" | "error" {
  switch (type) {
    case "Component": return "primary";
    case "Vault": return "success";
    case "Resource": return "secondary";
    case "NFT": return "info";
    case "Commitment": return "warning";
    case "Transaction Receipt": return "error";
    case "Template": return "info";
    case "Utxo": return "success";
    case "VnFeePool": return "secondary";
    default: return "primary";
  }
}

export default function Inputs({ data }: { data: SubstateRequirement[] }) {
  if (!data || data.length === 0) {
    return (
      <Box sx={{ p: 3, textAlign: "center" }}>
        <Typography variant="body2" color="text.secondary">
          No inputs required for this transaction
        </Typography>
      </Box>
    );
  }
  return (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <TableCell>Substate ID</TableCell>
            <TableCell>Type</TableCell>
            <TableCell>Version</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {data.map((item: SubstateRequirement, index: number) => {
            const substateId = substateIdToString(item.substate_id);
            const type = getSubstateType(substateId);
            return (
              <TableRow key={index}>
                <DataTableCell>
                  <Stack direction="row" alignItems="center">
                    <Typography variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
                      {substateId}
                    </Typography>
                    <CopyToClipboard copy={substateId} />
                  </Stack>
                </DataTableCell>
                <DataTableCell>
                  <Chip label={type} size="small" color={getTypeColor(type)} variant="outlined" />
                </DataTableCell>
                <DataTableCell>
                  {item.version !== null ? (
                    <Chip label={`v${item.version}`} size="small" color="default" variant="outlined" />
                  ) : (
                    <Chip label="Latest" size="small" color="success" variant="outlined" />
                  )}
                </DataTableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
