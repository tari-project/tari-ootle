// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
  Stack,
} from "@mui/material";
import { useState } from "react";
import { useStealthUtxosList } from "@/services/api/hooks/useAccounts";
import { Account, OutputStatus, ResourceAddress } from "@tari-project/typescript-bindings";
import FetchStatusCheck from "@/components/FetchStatusCheck";
import { DataTableCell } from "@components/StyledComponents";
import StatusChip from "./components/StatusChip";
import { useAccountsGetBalances } from "@/services/api/hooks/useAccounts";
import { substateIdToString } from "@utils/helpers";
import {
  emptyRows,
  handleChangePage,
  handleChangeRowsPerPage,
  bigintToDecimalString,
  shortenString,
} from "@utils/helpers";
import CopyToClipboard from "@components/CopyToClipboard";
import PlaceHolder from "./components/PlaceHolder";
import SortableHeader from "./components/SortableHeader";
import useCurrencyStore from "@store/currencyStore";

function StealthUtxoList({ account }: { account: Account }) {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [statusFilter, setStatusFilter] = useState<OutputStatus | "all">("all");
  const { data: balancesData } = useAccountsGetBalances(substateIdToString(account.component_address));
  const { currencySymbol } = useCurrencyStore();

  const stealthResources = balancesData?.balances?.filter((balance) => balance.resource_type === "Stealth") || [];

  const resourceToUse = stealthResources[0]?.resource_address;

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

  const { data, isLoading, isError, error } = useStealthUtxosList(
    account.component_address,
    resourceToUse!,
    statusFilter === "all" ? null : statusFilter,
  );

  const columnWidths = {
    1: "30%",
    2: "20%",
    3: "20%",
    4: "10%",
    5: "10%",
    6: "10%",
  };

  if (!resourceToUse) {
    return (
      <Stack minHeight={300} alignItems="center" justifyContent="center">
        <PlaceHolder status="empty" />
      </Stack>
    );
  }

  return (
    <Stack minHeight={300}>
      <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message || "Error fetching data"}>
        <TableContainer>
          <Table>
            <TableHead>
              <TableRow>
                <TableCell width={columnWidths[1]}>ID</TableCell>
                <TableCell width={columnWidths[2]}>Value</TableCell>
                <TableCell width={columnWidths[3]}>
                  <SortableHeader
                    title="Status"
                    currentFilter={statusFilter}
                    onFilterChange={setStatusFilter}
                    getDisplayName={getStatusDisplayName}
                  />
                </TableCell>
                <TableCell width={columnWidths[4]}>Burnt</TableCell>
                <TableCell width={columnWidths[5]}>Frozen</TableCell>
                <TableCell width={columnWidths[6]}>On Chain</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {data?.utxos && data.utxos.length > 0 ? (
                <>
                  {data.utxos.slice(page * rowsPerPage, page * rowsPerPage + rowsPerPage).map((utxo, index) => (
                    <TableRow key={`${utxo.address.id}-${index}`}>
                      <DataTableCell>
                        {shortenString(utxo.address.id)}
                        <CopyToClipboard copy={utxo.address.id} />
                      </DataTableCell>
                      <DataTableCell>{bigintToDecimalString(utxo.value, 6)} {currencySymbol}</DataTableCell>
                      <DataTableCell>
                        <StatusChip status={utxo.status} />
                      </DataTableCell>
                      <DataTableCell>{utxo.is_burnt ? "Yes" : "No"}</DataTableCell>
                      <DataTableCell>{utxo.is_frozen ? "Yes" : "No"}</DataTableCell>
                      <DataTableCell>{utxo.is_on_chain ? "Yes" : "No"}</DataTableCell>
                    </TableRow>
                  ))}
                  {emptyRows(page, rowsPerPage, data.utxos) > 0 && (
                    <TableRow
                      style={{
                        height: 57 * emptyRows(page, rowsPerPage, data.utxos),
                      }}
                    >
                      <TableCell colSpan={6} />
                    </TableRow>
                  )}
                </>
              ) : (
                !isLoading && (
                  <TableRow>
                    <TableCell colSpan={6}>
                      <PlaceHolder status="empty" utxoStatus={statusFilter === "all" ? undefined : statusFilter} />
                    </TableCell>
                  </TableRow>
                )
              )}
            </TableBody>
          </Table>
          {data?.utxos && data.utxos.length > 0 && (
            <TablePagination
              rowsPerPageOptions={[10, 25, 50]}
              component="div"
              count={data.utxos.length}
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

export default StealthUtxoList;
