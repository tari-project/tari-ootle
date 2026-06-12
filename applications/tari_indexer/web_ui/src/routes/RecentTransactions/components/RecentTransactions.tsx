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

import { ChevronRight } from "@mui/icons-material";
import { Chip, Stack } from "@mui/material";
import Fade from "@mui/material/Fade";
import IconButton from "@mui/material/IconButton";
import { useTheme } from "@mui/material/styles";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TablePagination from "@mui/material/TablePagination";
import TableRow from "@mui/material/TableRow";
import useMediaQuery from "@mui/material/useMediaQuery";
import { TransactionEntry } from "@tari-project/ootle-ts-bindings";
import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { useListRecentTransactions } from "../../../api/hooks/useTransactions";
import FetchStatusCheck from "../../../Components/FetchStatusCheck";
import StatusChip from "../../../Components/StatusChip";
import { DataTableCell } from "../../../Components/StyledComponents";
import { CURRENCY } from "../../../utils/constants";
import { formatCurrency } from "../../../utils/helpers";
import { shortenString } from "../../VN/Components/helpers";
import TransactionFilter from "./SearchFilter";
import TimeChip from "./TimeChip";

type ExtendedTransactionEntry = TransactionEntry & { id: string; show?: boolean };

function TransactionRow({ entry }: { entry: ExtendedTransactionEntry }) {
  const { transaction_id, created_at, summary, rejected_reason } = entry;
  const theme = useTheme();
  const isSm = useMediaQuery(theme.breakpoints.down("sm"));
  const shortLen = isSm ? 4 : 8;

  return (
    <TableRow>
      <DataTableCell>
        <Stack
          direction={{ xs: "column", sm: "row" }}
          spacing={isSm ? 0.1 : 1.5}
          alignItems={isSm ? "flex-start" : "center"}
        >
          <Link
            to={`/transactions/${transaction_id}`}
            style={{ textDecoration: "none", color: theme.palette.text.secondary }}
          >
            {shortenString(transaction_id, shortLen, shortLen)}
          </Link>
          <TimeChip timestamp={created_at} />
        </Stack>
      </DataTableCell>
      <DataTableCell>
        {summary ? (
          <StatusChip status="Commit" feeOnly={summary.outcome === "FeeIntentCommit"} showTitle={true} />
        ) : rejected_reason != null ? (
          <Chip label="Rejected" color="error" size="small" variant="outlined" title={rejected_reason} />
        ) : (
          <Chip label="Pending" color="warning" size="small" variant="outlined" />
        )}
      </DataTableCell>
      <DataTableCell>
        {summary ? formatCurrency(summary.total_fees_paid, CURRENCY.DECIMALS, CURRENCY.SYMBOL) : "--"}
      </DataTableCell>
      <DataTableCell>
        <IconButton
          component={Link}
          to={`/transactions/${transaction_id}`}
          style={{ color: theme.palette.text.secondary }}
        >
          <ChevronRight />
        </IconButton>
      </DataTableCell>
    </TableRow>
  );
}

function RecentTransactions() {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [filteredTransactions, setFilteredTransactions] = useState<ExtendedTransactionEntry[]>([]);

  const { data, isLoading, isError, error } = useListRecentTransactions({
    last_id: null,
    limit: 50,
  });

  const transactions = useMemo(
    () => (data?.transactions || []).map((tx) => ({ ...tx, id: tx.transaction_id })),
    [data?.transactions],
  );

  const visibleTransactions = filteredTransactions.filter((tx) => tx.show !== false);
  const paginatedTransactions = visibleTransactions.slice(page * rowsPerPage, page * rowsPerPage + rowsPerPage);

  const handleChangePage = (_event: React.MouseEvent<HTMLButtonElement> | null, newPage: number) => {
    setPage(newPage);
  };
  const handleChangeRowsPerPage = (event: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
    setRowsPerPage(parseInt(event.target.value, 10));
    setPage(0);
  };

  return (
    <FetchStatusCheck
      isLoading={isLoading}
      isError={isError}
      errorMessage={error ? error.message : "Error fetching transaction details."}
    >
      <Stack spacing={1}>
        <TransactionFilter
          setPage={setPage}
          stateObject={transactions}
          setStateObject={setFilteredTransactions}
          filterItems={[
            {
              title: "Transaction ID",
              value: "id",
              filterFn: (value, row) => row.transaction_id.toLowerCase().includes(value.toLowerCase()),
            },
          ]}
          placeholder="Search transactions"
          defaultSearch="id"
        />
        <Fade in={!isLoading && !isError}>
          <TableContainer>
            <Table>
              <TableHead>
                <TableRow>
                  <TableCell>Transaction Hash</TableCell>
                  <TableCell>Status</TableCell>
                  <TableCell>Total Fees</TableCell>
                  <TableCell>Details</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {paginatedTransactions.map((entry) => (
                  <TransactionRow key={entry.transaction_id} entry={entry} />
                ))}
              </TableBody>
            </Table>
            <TablePagination
              component="div"
              count={visibleTransactions.length}
              page={page}
              onPageChange={handleChangePage}
              rowsPerPage={rowsPerPage}
              onRowsPerPageChange={handleChangeRowsPerPage}
              rowsPerPageOptions={[5, 10, 25, 50]}
            />
          </TableContainer>
        </Fade>
      </Stack>
    </FetchStatusCheck>
  );
}

export default RecentTransactions;
