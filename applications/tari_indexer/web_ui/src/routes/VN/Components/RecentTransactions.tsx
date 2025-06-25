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

import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { renderJson } from "../../../utils/helpers";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import { DataTableCell, CodeBlock, AccordionIconButton } from "../../../Components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import Collapse from "@mui/material/Collapse";
import Typography from "@mui/material/Typography";
import { listRecentTransactions } from "../../../utils/json_rpc";
import { TransactionEntry } from "@tari-project/typescript-bindings";

function RowData(props: { data: TransactionEntry }) {
  const [open1, setOpen1] = useState(false);
  const [open2, setOpen2] = useState(false);

  const { transaction_id, transaction: tx } = props.data;
  const transaction = tx.V1.body.transaction;

  return (
    <>
      <TableRow sx={{ borderBottom: "none" }}>
        <DataTableCell
          sx={{
            borderBottom: "none",
          }}
        >
          {transaction_id}
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: "none", textAlign: "center" }}>
          <AccordionIconButton
            open={open1}
            aria-label="expand row"
            size="small"
            onClick={() => {
              setOpen1(!open1);
              setOpen2(false);
            }}
          >
            {open1 ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: "none", textAlign: "center" }}>
          <AccordionIconButton
            open={open2}
            aria-label="expand row"
            size="small"
            onClick={() => {
              setOpen2(!open2);
              setOpen1(false);
            }}
          >
            {open2 ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell
          style={{
            paddingBottom: 0,
            paddingTop: 0,
            borderBottom: "none",
          }}
          colSpan={4}
        >
          <Collapse in={open1} timeout="auto" unmountOnExit>
            <CodeBlock style={{ marginBottom: "10px" }}>{renderJson(transaction.fee_instructions)}</CodeBlock>
          </Collapse>
        </DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell style={{ paddingBottom: 0, paddingTop: 0 }} colSpan={4}>
          <Collapse in={open2} timeout="auto" unmountOnExit>
            <CodeBlock style={{ marginBottom: "10px" }}>{renderJson(transaction.instructions)}</CodeBlock>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

function RecentTransactions() {
  const [recentTransactions, setRecentTransactions] = useState<TransactionEntry[]>([]);

  useEffect(() => {
    listRecentTransactions({
      last_id: null,
      limit: 50,
    }).then((resp) => {
      console.log("Response: ", resp);
      setRecentTransactions(
        // Display from newest to oldest by reversing
        resp.transactions,
      );
    });
  }, []);

  if (recentTransactions === undefined) {
    return <Typography variant="h4">Recent transactions ... loading</Typography>;
  }

  return (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <TableCell>
              <div
                style={{
                  display: "flex",
                  justifyContent: "flex-start",
                  alignItems: "center",
                  gap: "5px",
                }}
              >
                Id
              </div>
            </TableCell>
            <TableCell style={{ textAlign: "center" }}>Fee Instructions</TableCell>
            <TableCell style={{ textAlign: "center" }}>Instructions</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {recentTransactions.map((data, i) => (
            <RowData key={i} data={data} />
          ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}

export default RecentTransactions;
