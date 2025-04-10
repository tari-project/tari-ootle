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
import { TableContainer, Table, TableRow, TableBody, Collapse } from "@mui/material";
import { renderJson } from "../../utils/helpers";
import { DataTableCell } from "../../Components/StyledComponents";
import { AccordionIconButton } from "../../Components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import { IoArrowDownCircle, IoArrowUpCircle } from "react-icons/io5";
import CodeBlockDialog from "../../Components/CodeBlock";
import { substateIdToString, SubstateRecord } from "@tari-project/typescript-bindings";
import { VersionedSubstateId } from "@tari-project/typescript-bindings/dist/types/VersionedSubstateId";
import { SubstateId } from "@tari-project/typescript-bindings/dist/types/SubstateId";
import { Substate } from "@tari-project/typescript-bindings/dist/types/Substate";

function UpSubstateRowData({ id, substate }: { id: SubstateId; substate: Substate }) {
  const [open, setOpen] = useState(false);
  const substateId = substateIdToString(id);
  return (
    <>
      <TableRow>
        <DataTableCell sx={{ borderBottom: "none", textAlign: "center" }}>
          <AccordionIconButton
            open={open}
            aria-label="expand row"
            size="small"
            onClick={() => {
              setOpen(!open);
            }}
          >
            {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
        <DataTableCell>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "flex-start",
              gap: "0.5rem",
            }}
          >
            <IoArrowUpCircle style={{ width: 22, height: 22, color: "#5F9C91" }} /> Up
          </div>
        </DataTableCell>
        <DataTableCell>{substateId}:{substate.version}</DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell style={{ paddingBottom: 0, paddingTop: 0 }} colSpan={4}>
          <Collapse in={open} timeout="auto" unmountOnExit>
            <CodeBlockDialog title="Substate">{renderJson(substate.substate)}</CodeBlockDialog>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

function DownSubstateRowData({ id }: { id: VersionedSubstateId }) {
  const substateId = substateIdToString(id.substate_id);
  return (
    <TableRow>
      <DataTableCell sx={{ borderBottom: "none", textAlign: "center" }} />
      <DataTableCell>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-start",
            gap: "0.5rem",
          }}
        >
          <IoArrowDownCircle style={{ width: 22, height: 22, color: "#ECA86A" }} />
          Down
        </div>
      </DataTableCell>
      <DataTableCell>{substateId}:{id.version}</DataTableCell>
    </TableRow>
  );
}


export default function Substates({ upData, downData }: {
  upData: [SubstateId, Substate][];
  downData: VersionedSubstateId[]
}) {
  // const down = data.Accept.down_substates;
  const up = upData;
  const down = downData;
  return (
    <TableContainer>
      <Table>
        <TableBody>
          {up.map(([id, substate], i) => {
            return <UpSubstateRowData id={id} substate={substate} key={i} />;
          })}
          {down.map((id, i) => {
            return <DownSubstateRowData id={id} key={i} />;
          })}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
