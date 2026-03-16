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
import { Box, Chip, Collapse, Stack, Table, TableBody, TableContainer, TableRow, Typography } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import { SubstateId, substateIdToString } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";
import CopyToClipboard from "../../../Components/CopyToClipboard";
import { AccordionIconButton, DataTableCell } from "../../../Components/StyledComponents";
import { CodeBlock } from "../../../Components/StyledComponents";
import { renderJson } from "../../../utils/helpers";
import { IoArrowDownCircle, IoArrowUpCircle } from "react-icons/io5";

interface SubstateRowProps {
  id: SubstateId;
  substate: any;
  state: "Up" | "Down";
  index: number;
}

function SubstateRow({ id, substate, state, index }: SubstateRowProps) {
  const [open, setOpen] = useState(false);
  const theme = useTheme();
  const substateId = substateIdToString(id);
  const version = substate !== null && substate !== undefined
    ? "v" + (typeof substate === "number" ? substate : substate.version)
    : "";

  return (
    <>
      <TableRow>
        <DataTableCell sx={{ borderBottom: "none" }}>
          <Stack direction="row" spacing={1} alignItems="center">
            {state === "Up" ? (
              <IoArrowUpCircle style={{ width: 22, height: 22, color: "#5F9C91" }} />
            ) : (
              <IoArrowDownCircle style={{ width: 22, height: 22, color: "#ECA86A" }} />
            )}
            <Chip label={state} size="small" color={state === "Up" ? "success" : "warning"} variant="outlined" />
          </Stack>
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: "none" }}>
          <Stack direction="row" alignItems="center">
            <Typography variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
              {substateId} {version}
            </Typography>
            <CopyToClipboard copy={substateId} />
          </Stack>
        </DataTableCell>
        <DataTableCell width={90} sx={{ borderBottom: "none", textAlign: "center" }}>
          <AccordionIconButton open={open} size="small" onClick={() => setOpen(!open)}>
            {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell colSpan={3} sx={{ padding: !open ? 0 : undefined }}>
          <Collapse in={open} timeout="auto" unmountOnExit>
            <CodeBlock sx={{ mb: 1 }}>{renderJson({ id, substate })}</CodeBlock>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

interface SubstatesContentProps {
  result: {
    Accept: {
      up_substates?: [SubstateId, any][];
      down_substates?: [SubstateId, any][];
    };
  };
}

export default function SubstatesContent({ result }: SubstatesContentProps) {
  const up = result.Accept.up_substates || [];
  const down = result.Accept.down_substates || [];

  if (up.length === 0 && down.length === 0) {
    return (
      <Box sx={{ p: 3, textAlign: "center" }}>
        <Typography variant="body2" color="text.secondary">
          No substate changes
        </Typography>
      </Box>
    );
  }

  return (
    <TableContainer>
      <Table>
        <TableBody>
          {up.map(([id, substate]: [SubstateId, any], index: number) => (
            <SubstateRow key={`up-${index}`} id={id} substate={substate} state="Up" index={index} />
          ))}
          {down.map(([id, substate]: [SubstateId, any], index: number) => (
            <SubstateRow key={`down-${index}`} id={id} substate={substate} state="Down" index={index} />
          ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
