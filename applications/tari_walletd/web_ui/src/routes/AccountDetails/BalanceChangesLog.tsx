//  Copyright 2026. The Tari Project
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
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, THE PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import FetchStatusCheck from "@components/FetchStatusCheck";
import { DataTableCell, InnerHeading, StyledPaper } from "@components/StyledComponents";
import { Table, TableBody, TableCell, TableContainer, TableHead, TableRow } from "@mui/material";
import Button from "@mui/material/Button";
import { useGetBalanceChanges } from "@api/hooks/useAccounts";
import { useTimeAgo } from "@hooks/useTimeAgo";
import { BalanceChangeEntry, BalanceChangeSource, ComponentAddress } from "@tari-project/ootle-ts-bindings";
import { Link } from "react-router-dom";
import { useState } from "react";
import CopyAddress from "@components/CopyAddress";

interface BalanceChangesLogProps {
  account: ComponentAddress;
}

const PAGE_SIZE = 20;

function SourceLabel({ source }: { source: BalanceChangeSource }) {
  if (source.type === "Transaction") {
    return (
      <Button component={Link} to={`/transactions/${source.transaction_id}`} size="small" variant="text">
        Transaction
      </Button>
    );
  }
  if (source.type === "Scan") {
    return <span>Scan</span>;
  }
  if (source.type === "Recovery") {
    return <span>Recovery</span>;
  }
  return <span>{source.type}</span>;
}

function TimestampCell({ created_at }: { created_at: string }) {
  const display = useTimeAgo(created_at);
  return <DataTableCell>{display}</DataTableCell>;
}

interface BalanceChangeRowProps {
  entry: BalanceChangeEntry;
  index: number;
}

function BalanceChangeRow({ entry, index }: BalanceChangeRowProps) {
  return (
    <TableRow>
      <TimestampCell created_at={entry.created_at} />
      <DataTableCell>
        <CopyAddress address={entry.resource_address} display={entry.resource_address.substring(0, 10) + "..."} />
      </DataTableCell>
      <DataTableCell>{entry.revealed_delta}</DataTableCell>
      <DataTableCell>{entry.confidential_delta}</DataTableCell>
      <DataTableCell>
        <SourceLabel source={entry.source} />
      </DataTableCell>
      <DataTableCell>
        {entry.transaction_id ? (
          <Button component={Link} to={`/transactions/${entry.transaction_id}`} size="small" variant="text">
            {entry.transaction_id.substring(0, 10)}...
          </Button>
        ) : (
          "--"
        )}
      </DataTableCell>
    </TableRow>
  );
}

function BalanceChangesLog({ account }: BalanceChangesLogProps) {
  const [page, setPage] = useState(0);

  const {
    data,
    isLoading,
    isError,
    error,
  } = useGetBalanceChanges(account, undefined, undefined, page * PAGE_SIZE, PAGE_SIZE);

  const changes = data?.changes || [];
  const total = Number(data?.total || 0);
  const hasMore = (page + 1) * PAGE_SIZE < total;

  return (
    <StyledPaper>
      <InnerHeading>Balance Changes</InnerHeading>
      <FetchStatusCheck
        isError={isError}
        errorMessage={(error as { message?: string })?.message || "Error fetching balance changes"}
        isLoading={isLoading && !data}
      >
        {changes.length === 0 ? (
          <TableContainer>
            <Table>
              <TableBody>
                <TableRow>
                  <DataTableCell>No balance changes yet</DataTableCell>
                </TableRow>
              </TableBody>
            </Table>
          </TableContainer>
        ) : (
          <>
            <TableContainer>
              <Table>
                <TableHead>
                  <TableRow>
                    <TableCell size="small">Time</TableCell>
                    <TableCell size="small">Resource</TableCell>
                    <TableCell size="small">Revealed Delta</TableCell>
                    <TableCell size="small">Conf. Delta</TableCell>
                    <TableCell size="small">Source</TableCell>
                    <TableCell size="small">Transaction</TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {changes.map((entry: BalanceChangeEntry, i: number) => (
                    <BalanceChangeRow key={`${entry.vault_address}_${entry.created_at}_${i}`} entry={entry} index={i} />
                  ))}
                </TableBody>
              </Table>
            </TableContainer>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginTop: 16 }}>
              <span>
                Showing {page * PAGE_SIZE + 1}-{page * PAGE_SIZE + changes.length} of {total}
              </span>
              <div style={{ display: "flex", gap: 8 }}>
                <Button disabled={page === 0} onClick={() => setPage(page - 1)} variant="outlined" size="small">
                  Previous
                </Button>
                <Button disabled={!hasMore} onClick={() => setPage(page + 1)} variant="outlined" size="small">
                  Next
                </Button>
              </div>
            </div>
          </>
        )}
      </FetchStatusCheck>
    </StyledPaper>
  );
}

export default BalanceChangesLog;
