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

import CodeBlockExpand from "@components/CodeBlock";
import CopyAddress from "@components/CopyAddress";
import { AccordionIconButton, DataTableCell } from "@components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import {
  Box,
  Chip,
  Collapse,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import { useTheme } from "@mui/material/styles";
import { Event, substateIdToString } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";

function renderPayloadField(key: string, value: any) {
  if (key === "amount" && typeof value === "string") {
    return (
      <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        <Typography variant="body2" color="text.secondary">
          Amount:
        </Typography>
        <Chip label={value} size="small" color="primary" variant="outlined" />
      </Box>
    );
  }

  if (key === "resource_type" && typeof value === "string") {
    return (
      <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        <Typography variant="body2" color="text.secondary">
          Resource Type:
        </Typography>
        <Chip label={value} size="small" color="secondary" variant="outlined" />
      </Box>
    );
  }

  if (key === "resource" || key === "resource_address") {
    return (
      <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        <Typography variant="body2" color="text.secondary">
          Resource:
        </Typography>
        <CopyAddress address={value} />
      </Box>
    );
  }

  if (key === "module_name" && typeof value === "string") {
    return (
      <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        <Typography variant="body2" color="text.secondary">
          Module:
        </Typography>
        <Chip label={value} size="small" color="info" variant="outlined" />
      </Box>
    );
  }

  return (
    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
      <Typography variant="body2" color="text.secondary">
        {key}:
      </Typography>
      <Typography variant="body2">{String(value)}</Typography>
    </Box>
  );
}

function renderPayload(payload: any) {
  if (!payload || typeof payload !== "object") {
    return <Typography variant="body2">{JSON.stringify(payload)}</Typography>;
  }

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      {Object.entries(payload).map(([key, value]) => (
        <Box key={key}>{renderPayloadField(key, value)}</Box>
      ))}
    </Box>
  );
}

function RowData({ substate_id, template_address, topic, payload }: Event, index: number) {
  const [open, setOpen] = useState(false);
  const theme = useTheme();
  return (
    <>
      <TableRow key={index}>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>{topic}</DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>
          {substate_id ? <CopyAddress address={substateIdToString(substate_id)} /> : "--"}
        </DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>
          {template_address ? <CopyAddress address={template_address} /> : "--"}
        </DataTableCell>
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
          colSpan={5}
        >
          <Collapse in={open} timeout="auto" unmountOnExit>
            <Box sx={{ p: 2, backgroundColor: theme.palette.accent.background, borderRadius: 1 }}>
              <Typography variant="subtitle2" sx={{ mb: 1 }}>
                Payload Details
              </Typography>
              {renderPayload(payload)}
              <Box sx={{ mt: 2 }}>
                <CodeBlockExpand title="Raw Payload" content={payload} />
              </Box>
            </Box>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

export default function Events({ data }: { data: Event[] }) {
  return (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <TableCell>Topic</TableCell>
            <TableCell>Substate Id</TableCell>
            <TableCell>Template Address</TableCell>
            <TableCell>Transaction Hash</TableCell>
            <TableCell width={90}>Details</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {data.map(({ substate_id, template_address, topic, payload }: Event, index: number) => {
            return (
              <RowData
                substate_id={substate_id}
                template_address={template_address}
                topic={topic}
                payload={payload}
                key={index}
              />
            );
          })}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
