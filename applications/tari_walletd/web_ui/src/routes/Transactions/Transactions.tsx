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

import { useGetAllTransactions } from "@api/hooks/useTransactions";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { DataTableCell } from "@components/StyledComponents";
import TransactionsStatusChip from "@components/TransactionsStatusChip";
import { ChevronRight } from "@mui/icons-material";
import { Stack } from "@mui/material";
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
import { Account, WalletTransaction } from "@tari-project/ootle-ts-bindings";
import { XTR_CURRENCY } from "@utils/currency";
import { emptyRows, formatCurrency, handleChangePage, handleChangeRowsPerPage, shortenString } from "@utils/helpers";
import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import TimeChip from "./TimeChip";

interface TransactionsProps {
  account?: Account | null;
}
export default function Transactions({ account }: TransactionsProps) {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const accountAddress = account?.component_address ?? null;
  const { data, isLoading, error, isError, isRefetching } = useGetAllTransactions({
    status: null,
    // Transactions are linked to the account(s) they involve at submission time, so filtering by account
    // works even for stealth transactions. When no account is given, all transactions are shown.
    account: accountAddress,
  });

  const transactions = data?.transactions;
  const totalTransactions = transactions?.length ?? 0;
  const maxPage = totalTransactions > 0 ? Math.ceil(totalTransactions / rowsPerPage) - 1 : 0;
  const clampedPage = Math.min(page, maxPage);
  const emptyRowCount = emptyRows(clampedPage, rowsPerPage, transactions);

  useEffect(() => {
    setPage(0);
  }, [accountAddress]);

  useEffect(() => {
    if (transactions !== undefined && page > maxPage) {
      setPage(maxPage);
    }
  }, [transactions, page, maxPage]);

  const theme = useTheme();
  const isSm = useMediaQuery(theme.breakpoints.down("sm"));

  const shortLen = isSm ? 4 : 6;
  return (
    <FetchStatusCheck
      isLoading={isLoading && !isRefetching}
      isError={isError}
      errorMessage={error?.message || "Error fetching data"}
    >
      <Fade in={!isLoading && !isError}>
        <TableContainer>
          <Table>
            <TableHead>
              <TableRow>
                <TableCell size="medium">Transaction Hash</TableCell>
                <TableCell size="medium">Status</TableCell>
                <TableCell size="medium">Total Fees</TableCell>
                <TableCell size="medium">Details</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {transactions
                ?.slice(clampedPage * rowsPerPage, clampedPage * rowsPerPage + rowsPerPage)
                .map((transaction: WalletTransaction) => {
                  const { finalize: result, status, id: hash } = transaction;
                  const { fee_receipt } = result || {};
                  return (
                    <TableRow key={hash}>
                      <DataTableCell>
                        <Stack
                          direction={{
                            xs: "column",
                            sm: "row",
                          }}
                          spacing={isSm ? 0.1 : 1.5}
                        >
                          <Link
                            to={`/transactions/${hash}`}
                            style={{
                              textDecoration: "none",
                              color: theme.palette.text.secondary,
                            }}
                          >
                            {shortenString(hash, shortLen, shortLen)}
                          </Link>
                          <TimeChip timestamp={transaction.last_update_time} />
                        </Stack>
                      </DataTableCell>
                      <DataTableCell>
                        <TransactionsStatusChip status={status} />
                      </DataTableCell>
                      <DataTableCell>
                        {fee_receipt?.total_fees_paid != null
                          ? formatCurrency(fee_receipt.total_fees_paid, XTR_CURRENCY)
                          : "--"}
                      </DataTableCell>
                      <DataTableCell>
                        <IconButton
                          component={Link}
                          to={`/transactions/${hash}`}
                          style={{
                            color: theme.palette.text.secondary,
                          }}
                        >
                          <ChevronRight />
                        </IconButton>
                      </DataTableCell>
                    </TableRow>
                  );
                })}
              {emptyRowCount > 0 && (
                <TableRow
                  style={{
                    height: 57 * emptyRowCount,
                  }}
                >
                  <TableCell colSpan={4} />
                </TableRow>
              )}
            </TableBody>
          </Table>
          {transactions && (
            <TablePagination
              rowsPerPageOptions={[10, 25, 50]}
              component="div"
              count={totalTransactions}
              rowsPerPage={rowsPerPage}
              page={clampedPage}
              onPageChange={(event, newPage) => handleChangePage(event, newPage, setPage)}
              onRowsPerPageChange={(event) => handleChangeRowsPerPage(event, setRowsPerPage, setPage)}
            />
          )}
        </TableContainer>
      </Fade>
    </FetchStatusCheck>
  );
}
