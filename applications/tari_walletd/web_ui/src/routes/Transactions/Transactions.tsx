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
import { useState } from "react";
import { Link } from "react-router-dom";
import TimeChip from "./TimeChip";

interface TransactionsProps {
  account?: Account;
}
export default function Transactions({ account: _ }: TransactionsProps) {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const { data, isLoading, error, isError, isRefetching } = useGetAllTransactions({
    status: null,
    // Some stealth transactions cannot be identified by component/public key in the wallet - so we fetch all transactions.
    // If this feature is badly needed, we can "tag" transactions as involving a specific account when they are created.
    component: null, //: account ? substateIdToString(account.address) : null,
    // Stealth transaction are not able to be identified by signer public key, so for simplicity we fetch all transactions.
    signer_public_key: null,
  });

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
                <TableCell size="small">Transaction Hash</TableCell>
                <TableCell size="small">Status</TableCell>
                <TableCell size="small">Total Fees</TableCell>
                <TableCell size="small">Details</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {data?.transactions
                ?.slice(page * rowsPerPage, page * rowsPerPage + rowsPerPage)
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
                        {fee_receipt?.total_fees_paid
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
              {emptyRows(page, rowsPerPage, data?.transactions) > 0 && (
                <TableRow
                  style={{
                    height: 57 * emptyRows(page, rowsPerPage, data?.transactions),
                  }}
                >
                  <TableCell colSpan={3} />
                </TableRow>
              )}
            </TableBody>
          </Table>
          {data?.transactions && (
            <TablePagination
              rowsPerPageOptions={[10, 25, 50]}
              component="div"
              count={data.transactions.length}
              rowsPerPage={rowsPerPage}
              page={page}
              onPageChange={(event, newPage) => handleChangePage(event, newPage, setPage)}
              onRowsPerPageChange={(event) => handleChangeRowsPerPage(event, setRowsPerPage, setPage)}
            />
          )}
        </TableContainer>
      </Fade>
    </FetchStatusCheck>
  );
}
