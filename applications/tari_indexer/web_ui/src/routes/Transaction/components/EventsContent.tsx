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

import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import {
  Box,
  Chip,
  Collapse,
  Stack,
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
import CopyToClipboard from "../../../Components/CopyToClipboard";
import { AccordionIconButton, DataTableCell } from "../../../Components/StyledComponents";

function EventRow({ event, index }: { event: Event; index: number }) {
  const [open, setOpen] = useState(false);
  const theme = useTheme();
  const { substate_id, template_address, topic, payload } = event;

  return (
    <>
      <TableRow>
        <DataTableCell sx={{ borderBottom: "none" }}>{topic}</DataTableCell>
        <DataTableCell sx={{ borderBottom: "none" }}>
          {substate_id ? (
            <Stack direction="row" alignItems="center">
              <Typography variant="body2" sx={{ fontFamily: "monospace" }}>
                {substateIdToString(substate_id)}
              </Typography>
              <CopyToClipboard copy={substateIdToString(substate_id)} />
            </Stack>
          ) : (
            "--"
          )}
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: "none" }}>
          {template_address ? (
            <Stack direction="row" alignItems="center">
              <Typography variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
                {template_address}
              </Typography>
              <CopyToClipboard copy={template_address} />
            </Stack>
          ) : (
            "--"
          )}
        </DataTableCell>
        <DataTableCell width={90} sx={{ borderBottom: "none", textAlign: "center" }}>
          <AccordionIconButton open={open} size="small" onClick={() => setOpen(!open)}>
            {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell colSpan={4} sx={{ padding: !open ? 0 : undefined }}>
          <Collapse in={open} timeout="auto" unmountOnExit>
            <Box sx={{ p: 2, backgroundColor: theme.palette.divider, borderRadius: 1 }}>
              <Typography variant="subtitle2" sx={{ mb: 1 }}>
                Payload
              </Typography>
              <Box sx={{ display: "flex", flexWrap: "wrap", gap: 1 }}>
                {payload && typeof payload === "object" ? (
                  Object.entries(payload).map(([key, value], i) => (
                    <Chip key={i} label={`${key}: ${value || "<null>"}`} size="small" variant="outlined" />
                  ))
                ) : (
                  <Typography variant="body2">{JSON.stringify(payload)}</Typography>
                )}
              </Box>
            </Box>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

export default function EventsContent({ data }: { data: Event[] }) {
  return (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <TableCell>Topic</TableCell>
            <TableCell>Substate ID</TableCell>
            <TableCell>Template Address</TableCell>
            <TableCell width={90}>Details</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {data.map((event, index) => (
            <EventRow key={index} event={event} index={index} />
          ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
