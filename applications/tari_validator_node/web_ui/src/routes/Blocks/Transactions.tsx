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

import { Table, TableBody, TableCell, TableContainer, TableHead, TableRow } from "@mui/material";
import type { LocalOnlyAtom, MultiShardAtom } from "@tari-project/ootle-ts-bindings";
import { Link } from "react-router-dom";
import CodeBlockDialog from "../../Components/CodeBlock";
import StatusChip from "../../Components/StatusChip";
import { renderJson } from "../../utils/helpers";

function Transaction({ transaction }: { transaction: MultiShardAtom | LocalOnlyAtom }) {
  const decision = transaction.decision !== null && typeof transaction.decision === "object" ? "Abort" : "Commit";
  return (
    <TableRow>
      <TableCell>
        <Link to={`/transactions/${encodeURIComponent(transaction.id)}`}>{transaction.id}</Link>
      </TableCell>
      <TableCell>
        <StatusChip status={decision} />
      </TableCell>
      <TableCell>
        {"evidence" in transaction && (
          <CodeBlockDialog title="Evidence" contentsWhenUnexpanded={<></>}>
            {renderJson(transaction.evidence)}
          </CodeBlockDialog>
        )}
      </TableCell>
      <TableCell>{transaction.leader_fee?.fee ?? "-"}</TableCell>
      <TableCell>{transaction.transaction_fee}</TableCell>
    </TableRow>
  );
}

export default function Transactions({ transactions }: { transactions: (MultiShardAtom | LocalOnlyAtom)[] }) {
  return (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <TableCell>Transaction ID</TableCell>
            <TableCell>Decision</TableCell>
            <TableCell>Evidence</TableCell>
            <TableCell>Leader fee</TableCell>
            <TableCell>Transaction fee</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {transactions.length === 0 ? (
            <TableRow>
              <TableCell colSpan={5} align="center">
                No transactions found
              </TableCell>
            </TableRow>
          ) : (
            transactions.map((tx) => <Transaction key={tx.id} transaction={tx} />)
          )}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
