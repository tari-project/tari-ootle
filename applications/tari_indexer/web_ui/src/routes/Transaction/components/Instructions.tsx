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
import { Box, Collapse, Table, TableBody, TableContainer, TableRow, Typography } from "@mui/material";
import type { Instruction } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";
import { AccordionIconButton, CodeBlock, DataTableCell } from "../../../Components/StyledComponents";
import { renderJson } from "../../../utils/helpers";

function InstructionRow({ data, index }: { data: Instruction; index: number }) {
  const [open, setOpen] = useState(false);
  const title = typeof data === "object" && data !== null ? Object.keys(data)[0] : "Unknown";
  return (
    <>
      <TableRow>
        <DataTableCell sx={{ borderBottom: "none" }}>{title}</DataTableCell>
        <DataTableCell width={90} sx={{ borderBottom: "none", textAlign: "center" }}>
          <AccordionIconButton open={open} size="small" onClick={() => setOpen(!open)}>
            {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell colSpan={2} sx={{ padding: !open ? 0 : undefined }}>
          <Collapse in={open} timeout="auto" unmountOnExit>
            <CodeBlock sx={{ mb: 1 }}>{renderJson(data)}</CodeBlock>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

export default function Instructions({ data }: { data: Instruction[] }) {
  if (!data?.length) {
    return (
      <Box sx={{ p: 3, textAlign: "center" }}>
        <Typography variant="body2" color="text.secondary">
          No instructions available
        </Typography>
      </Box>
    );
  }
  return (
    <TableContainer>
      <Table>
        <TableBody>
          {data.map((item, index) => (
            <InstructionRow key={index} data={item} index={index} />
          ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
