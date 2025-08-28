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

import { useState } from "react";
import {
  TableContainer,
  Table,
  TableHead,
  TableRow,
  TableCell,
  TableBody,
  Collapse,
  Box,
  Typography,
  Chip,
} from "@mui/material";
import { DataTableCell, AccordionIconButton } from "../../Components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import CodeBlockExpand from "../../Components/CodeBlock";
import { useTheme } from "@mui/material/styles";

function ResultRowData({ result, index }: { result: any; index: number }) {
  const [open, setOpen] = useState(false);
  const theme = useTheme();

  const returnTypeLabel =
    typeof result.return_type === "string" ? result.return_type : result.return_type?.Other?.name || "Unknown";

  const renderValue = () => {
    if (!result.indexed?.value) return null;

    if (typeof result.indexed.value === "string") {
      return result.indexed.value;
    }

    if (result.indexed.value.Tag) {
      return `Tag ${result.indexed.value.Tag[0]}: ${JSON.stringify(result.indexed.value.Tag[1])}`;
    }

    return JSON.stringify(result.indexed.value);
  };

  const renderIndexedData = () => {
    if (!result.indexed?.indexed) return null;

    const indexed = result.indexed.indexed;
    const sections = [];

    if (indexed.bucket_ids?.length > 0) {
      sections.push(
        <Box key="bucket_ids" sx={{ mb: 1 }}>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 0.5 }}>
            Bucket IDs:
          </Typography>
          <Box sx={{ display: "flex", gap: 0.5, flexWrap: "wrap" }}>
            {indexed.bucket_ids.map((id: any, idx: number) => (
              <Chip key={idx} label={id} size="small" color="primary" variant="outlined" />
            ))}
          </Box>
        </Box>,
      );
    }

    if (indexed.component_addresses?.length > 0) {
      sections.push(
        <Box key="component_addresses" sx={{ mb: 1 }}>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 0.5 }}>
            Component Addresses:
          </Typography>
          <Box sx={{ display: "flex", gap: 0.5, flexWrap: "wrap" }}>
            {indexed.component_addresses.map((addr: any, idx: number) => (
              <Chip key={idx} label={addr} size="small" color="info" variant="outlined" />
            ))}
          </Box>
        </Box>,
      );
    }

    if (indexed.resource_addresses?.length > 0) {
      sections.push(
        <Box key="resource_addresses" sx={{ mb: 1 }}>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 0.5 }}>
            Resource Addresses:
          </Typography>
          <Box sx={{ display: "flex", gap: 0.5, flexWrap: "wrap" }}>
            {indexed.resource_addresses.map((addr: any, idx: number) => (
              <Chip key={idx} label={addr} size="small" color="success" variant="outlined" />
            ))}
          </Box>
        </Box>,
      );
    }

    if (indexed.vault_ids?.length > 0) {
      sections.push(
        <Box key="vault_ids" sx={{ mb: 1 }}>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 0.5 }}>
            Vault IDs:
          </Typography>
          <Box sx={{ display: "flex", gap: 0.5, flexWrap: "wrap" }}>
            {indexed.vault_ids.map((id: any, idx: number) => (
              <Chip key={idx} label={id} size="small" color="warning" variant="outlined" />
            ))}
          </Box>
        </Box>,
      );
    }

    if (indexed.non_fungible_addresses?.length > 0) {
      sections.push(
        <Box key="non_fungible_addresses" sx={{ mb: 1 }}>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 0.5 }}>
            NFT Addresses:
          </Typography>
          <Box sx={{ display: "flex", gap: 0.5, flexWrap: "wrap" }}>
            {indexed.non_fungible_addresses.map((addr: any, idx: number) => (
              <Chip key={idx} label={addr} size="small" color="secondary" variant="outlined" />
            ))}
          </Box>
        </Box>,
      );
    }

    if (indexed.proof_ids?.length > 0) {
      sections.push(
        <Box key="proof_ids" sx={{ mb: 1 }}>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 0.5 }}>
            Proof IDs:
          </Typography>
          <Box sx={{ display: "flex", gap: 0.5, flexWrap: "wrap" }}>
            {indexed.proof_ids.map((id: any, idx: number) => (
              <Chip key={idx} label={id} size="small" color="error" variant="outlined" />
            ))}
          </Box>
        </Box>,
      );
    }

    return sections.length > 0 ? sections : null;
  };

  return (
    <>
      <TableRow key={index}>
        <DataTableCell width={150} sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>Result #{index + 1}</DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>
          <Chip label={returnTypeLabel} size="small" color="secondary" variant="outlined" />
        </DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>{renderValue() || "--"}</DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none", textAlign: "center" }}>
          <AccordionIconButton
            aria-label="expand row"
            size="small"
            onClick={() => {
              setOpen(!open);
            }}
          >
            {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell
          style={{
            paddingBottom: theme.spacing(1),
            paddingTop: 0,
            borderBottom: "none",
          }}
          colSpan={4}
        >
          <Collapse in={open} timeout="auto" unmountOnExit>
            <Box sx={{ p: 2, backgroundColor: theme.palette.accent.background, borderRadius: 1 }}>
              {renderIndexedData() && (
                <Box sx={{ mb: 2 }}>
                  <Typography variant="subtitle2" sx={{ mb: 1 }}>
                    Indexed Data
                  </Typography>
                  {renderIndexedData()}
                </Box>
              )}
              <CodeBlockExpand title="Raw Execution Result" content={result} />
            </Box>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
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
            <TableCell width={90}>Details</TableCell>
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
