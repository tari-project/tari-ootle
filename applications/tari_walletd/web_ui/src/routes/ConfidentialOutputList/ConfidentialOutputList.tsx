//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import FetchStatusCheck from "@/components/FetchStatusCheck";
import { useAccountsGetBalances, useConfidentialOutputsList } from "@/services/api/hooks/useAccounts";
import CopyToClipboard from "@components/CopyToClipboard";
import { Memo } from "@components/Memo";
import { DataTableCell } from "@components/StyledComponents";
import {
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
} from "@mui/material";
import { Account, OutputStatus, TARI_TOKEN } from "@tari-project/ootle-ts-bindings";
import {
  bigintToDecimalString,
  emptyRows,
  handleChangePage,
  handleChangeRowsPerPage,
  shortenString,
  substateIdToString,
} from "@utils/helpers";
import { useState } from "react";
import { useParams } from "react-router-dom";
import PlaceHolder from "@routes/StealthUtxoList/components/PlaceHolder";
import SortableHeader from "@routes/StealthUtxoList/components/SortableHeader";
import StatusChip from "@routes/StealthUtxoList/components/StatusChip";

function ConfidentialOutputList({ account }: { account: Account }) {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [statusFilter, setStatusFilter] = useState<OutputStatus | "all">("all");
  const { data: balancesData } = useAccountsGetBalances(substateIdToString(account.component_address));
  const params = useParams();
  const resourceAddress = params.resource_address || TARI_TOKEN;

  const resourceBalance = balancesData?.balances?.find((balance) => balance.resource_address === resourceAddress);
  const currencySymbol = resourceBalance ? resourceBalance.token_symbol || "" : "";
  const divisibility = resourceBalance ? resourceBalance.divisibility : 6;

  const getStatusDisplayName = (status: OutputStatus | "all") => {
    switch (status) {
      case "all":
        return "All";
      case "LockedForSpend":
        return "Locked for Spend";
      case "LockedUnconfirmed":
        return "Locked Unconfirmed";
      default:
        return status;
    }
  };

  const { data, isLoading, isError, error } = useConfidentialOutputsList(
    account.component_address,
    resourceAddress,
    statusFilter === "all" ? null : statusFilter,
  );

  const columnWidths = {
    1: "30%",
    2: "20%",
    3: "20%",
    4: "30%",
  };

  return (
    <Stack minHeight={300}>
      <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message || "Error fetching data"}>
        <TableContainer>
          <Table>
            <TableHead>
              <TableRow>
                <TableCell width={columnWidths[1]}>Commitment</TableCell>
                <TableCell width={columnWidths[2]}>Value</TableCell>
                <TableCell width={columnWidths[3]}>
                  <SortableHeader
                    title="Status"
                    currentFilter={statusFilter}
                    onFilterChange={setStatusFilter}
                    getDisplayName={getStatusDisplayName}
                  />
                </TableCell>
                <TableCell width={columnWidths[4]}>Encrypted Memo</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {data?.outputs && data.outputs.length > 0 ? (
                <>
                  {data.outputs.slice(page * rowsPerPage, page * rowsPerPage + rowsPerPage).map((output, index) => (
                    <TableRow key={`${output.address.commitment}-${index}`}>
                      <DataTableCell>
                        {shortenString(output.address.commitment)}
                        <CopyToClipboard copy={output.address.commitment} />
                      </DataTableCell>
                      <DataTableCell>
                        {bigintToDecimalString(output.value, divisibility)} {currencySymbol}
                      </DataTableCell>
                      <DataTableCell>
                        <StatusChip status={output.status} />
                      </DataTableCell>
                      <DataTableCell>
                        <Memo memo={output.memo} />
                      </DataTableCell>
                    </TableRow>
                  ))}
                  {emptyRows(page, rowsPerPage, data.outputs) > 0 && (
                    <TableRow
                      style={{
                        height: 57 * emptyRows(page, rowsPerPage, data.outputs),
                      }}
                    >
                      <TableCell colSpan={4} />
                    </TableRow>
                  )}
                </>
              ) : (
                !isLoading && (
                  <TableRow>
                    <TableCell colSpan={4}>
                      <PlaceHolder
                        status="empty"
                        utxoStatus={statusFilter === "all" ? undefined : statusFilter}
                        shortName="outputs"
                        longName="confidential outputs"
                      />
                    </TableCell>
                  </TableRow>
                )
              )}
            </TableBody>
          </Table>
          {data?.outputs && data.outputs.length > 0 && (
            <TablePagination
              rowsPerPageOptions={[10, 25, 50]}
              component="div"
              count={data.outputs.length}
              rowsPerPage={rowsPerPage}
              page={page}
              onPageChange={(event, newPage) => handleChangePage(event, newPage, setPage)}
              onRowsPerPageChange={(event) => handleChangeRowsPerPage(event, setRowsPerPage, setPage)}
            />
          )}
        </TableContainer>
      </FetchStatusCheck>
    </Stack>
  );
}

export default ConfidentialOutputList;
