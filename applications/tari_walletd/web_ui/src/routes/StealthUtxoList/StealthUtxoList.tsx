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
  MenuItem,
  Stack,
  IconButton,
  Menu,
  Chip,
} from "@mui/material";
import { useState } from "react";
import { useStealthUtxosList } from "@/services/api/hooks/useAccounts";
import { Account, OutputStatus } from "@tari-project/typescript-bindings";
import { XTR_RESOURCE } from "@utils/constants";
import FetchStatusCheck from "@/components/FetchStatusCheck";
import { DataTableCell } from "@components/StyledComponents";
import StatusChip from "./components/StatusChip";
import {
  emptyRows,
  handleChangePage,
  handleChangeRowsPerPage,
  bigintToDecimalString,
  shortenString,
} from "@utils/helpers";
import CopyToClipboard from "@components/CopyToClipboard";
import PlaceHolder from "./components/PlaceHolder";
import { IoEllipsisVerticalOutline, IoCloseOutline } from "react-icons/io5";

function StealthUtxoList({ account }: { account: Account }) {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [statusFilter, setStatusFilter] = useState<OutputStatus | "all">("all");
  const [menuAnchorEl, setMenuAnchorEl] = useState<null | HTMLElement>(null);

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

  const handleMenuOpen = (event: React.MouseEvent<HTMLElement>) => {
    setMenuAnchorEl(event.currentTarget);
  };

  const handleMenuClose = () => {
    setMenuAnchorEl(null);
  };

  const handleStatusSelect = (status: OutputStatus | "all") => {
    setStatusFilter(status);
    handleMenuClose();
  };
  const { data, isLoading, isError, error } = useStealthUtxosList(
    account.component_address,
    XTR_RESOURCE,
    statusFilter === "all" ? null : statusFilter,
  );

  return (
    <Stack minHeight={300}>
      <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message || "Error fetching data"}>
        <TableContainer>
          <Table>
            <TableHead>
              <TableRow>
                <TableCell>Transaction Hash</TableCell>
                <TableCell>Value</TableCell>
                <TableCell>
                  <Stack direction="row" alignItems="center" justifyContent="space-between" spacing={1} maxWidth={130}>
                    <Stack direction="row" alignItems="center" spacing={0.5}>
                      <span>Status</span>
                      {statusFilter !== "all" && (
                        <Chip
                          label={getStatusDisplayName(statusFilter)}
                          size="small"
                          variant="outlined"
                          onDelete={() => setStatusFilter("all")}
                          deleteIcon={<IoCloseOutline style={{ fontSize: 12 }} />}
                          sx={{ fontSize: 11 }}
                        />
                      )}
                    </Stack>
                    <IconButton size="small" onClick={handleMenuOpen} sx={{ p: 0.25 }}>
                      <IoEllipsisVerticalOutline style={{ fontSize: 14 }} />
                    </IconButton>
                    <Menu anchorEl={menuAnchorEl} open={Boolean(menuAnchorEl)} onClose={handleMenuClose}>
                      <MenuItem onClick={() => handleStatusSelect("all")}>All</MenuItem>
                      <MenuItem onClick={() => handleStatusSelect("Unspent")}>Unspent</MenuItem>
                      <MenuItem onClick={() => handleStatusSelect("Spent")}>Spent</MenuItem>
                      <MenuItem onClick={() => handleStatusSelect("LockedForSpend")}>Locked for Spend</MenuItem>
                      <MenuItem onClick={() => handleStatusSelect("LockedUnconfirmed")}>Locked Unconfirmed</MenuItem>
                      <MenuItem onClick={() => handleStatusSelect("Invalid")}>Invalid</MenuItem>
                    </Menu>
                  </Stack>
                </TableCell>
                <TableCell>Burnt</TableCell>
                <TableCell>Frozen</TableCell>
                <TableCell>On Chain</TableCell>
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
                      <DataTableCell>{bigintToDecimalString(utxo.value, 6)} XTR</DataTableCell>
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
