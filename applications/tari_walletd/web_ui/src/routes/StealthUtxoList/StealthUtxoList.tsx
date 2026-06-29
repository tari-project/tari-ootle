// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import FetchStatusCheck from "@/components/FetchStatusCheck";
import { useAccountsGetBalances, useStealthUtxosList } from "@/services/api/hooks/useAccounts";
import CopyToClipboard from "@components/CopyToClipboard";
import { Memo } from "@components/Memo";
import { DataTableCell } from "@components/StyledComponents";
import {
  IconButton,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
  Tooltip,
} from "@mui/material";
import { Account, OutputStatus, TARI_TOKEN, UtxoInfo } from "@tari-project/ootle-ts-bindings";
import {
  bigintToDecimalString,
  emptyRows,
  handleChangePage,
  handleChangeRowsPerPage,
  shortenString,
  substateIdToString,
} from "@utils/helpers";
import { useState } from "react";
import { IoExpandOutline } from "react-icons/io5";
import { useParams } from "react-router-dom";
import PlaceHolder from "./components/PlaceHolder";
import { SenderAddress } from "./components/SenderAddress";
import SortableHeader from "./components/SortableHeader";
import StatusChip from "./components/StatusChip";
import UtxoDetailsDialog from "./components/UtxoDetailsDialog";

function StealthUtxoList({ account }: { account: Account }) {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [statusFilter, setStatusFilter] = useState<OutputStatus | "all">("all");
  const [selectedUtxo, setSelectedUtxo] = useState<UtxoInfo | null>(null);
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

  const { data, isLoading, isError, error } = useStealthUtxosList(
    account.component_address,
    resourceAddress,
    statusFilter === "all" ? null : statusFilter,
  );

  const columnWidths = {
    1: "15%",
    2: "20%",
    3: "15%",
    4: "25%",
    5: "10%",
    6: "10%",
    7: "5%",
  };

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
                <TableCell width={columnWidths[4]}>Encrypted Memo</TableCell>
                <TableCell width={columnWidths[5]}>Burnt</TableCell>
                <TableCell width={columnWidths[6]}>Frozen</TableCell>
                <TableCell width={columnWidths[7]} align="center">
                  Details
                </TableCell>
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
                      <DataTableCell>
                        {bigintToDecimalString(utxo.value, divisibility)} {currencySymbol}
                      </DataTableCell>
                      <DataTableCell>
                        <StatusChip status={utxo.status} tooltip={JSON.stringify(utxo.auth)} />
                      </DataTableCell>
                      <DataTableCell>
                        {utxo.sender_address ? (
                          <SenderAddress address={utxo.sender_address} />
                        ) : (
                          <Memo memo={utxo.memo} />
                        )}
                      </DataTableCell>
                      <DataTableCell>{utxo.is_burnt ? "Yes" : "No"}</DataTableCell>
                      <DataTableCell>{utxo.is_frozen ? "Yes" : "No"}</DataTableCell>
                      <DataTableCell align="center">
                        <Tooltip title="View details" arrow>
                          <IconButton size="small" aria-label="view utxo details" onClick={() => setSelectedUtxo(utxo)}>
                            <IoExpandOutline style={{ height: 16, width: 16 }} />
                          </IconButton>
                        </Tooltip>
                      </DataTableCell>
                    </TableRow>
                  ))}
                  {emptyRows(page, rowsPerPage, data.utxos) > 0 && (
                    <TableRow
                      style={{
                        height: 57 * emptyRows(page, rowsPerPage, data.utxos),
                      }}
                    >
                      <TableCell colSpan={7} />
                    </TableRow>
                  )}
                </>
              ) : (
                !isLoading && (
                  <TableRow>
                    <TableCell colSpan={7}>
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
      <UtxoDetailsDialog utxo={selectedUtxo} open={selectedUtxo !== null} onClose={() => setSelectedUtxo(null)} />
    </Stack>
  );
}

export default StealthUtxoList;
