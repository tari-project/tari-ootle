//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import CopyAddress from "@components/CopyAddress";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { DataTableCell } from "@components/StyledComponents";
import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Link as MuiLink,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
  Typography,
} from "@mui/material";
import type { BalanceChange, ComponentAddress, ResourceAddress } from "@tari-project/ootle-ts-bindings";
import { shortenString } from "@tari-project/ootle-ts-bindings";
import { Currency } from "@utils/currency";
import { bigintToDecimalString, formatCurrency, formatTimestamp, validateHash } from "@utils/helpers";
import { useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useAccountsGetBalanceChanges } from "../../services/api/hooks/useAccounts";

interface BalanceChangeHistoryProps {
  accountAddress: ComponentAddress;
  resourceAddress?: ResourceAddress;
}

function formatSignedDelta(delta: string, currency: Currency): string {
  const value = BigInt(delta);
  const sign = value > 0n ? "+" : value < 0n ? "−" : "";
  const magnitude = value < 0n ? -value : value;
  const symbol = currency.symbol ? ` ${currency.symbol}` : "";
  return `${sign}${bigintToDecimalString(magnitude, currency.decimals)}${symbol}`;
}

function ChangeValues({ change }: { change: BalanceChange }) {
  const currency = { symbol: change.token_symbol, decimals: change.divisibility };
  const values = [
    { label: "Revealed", value: change.revealed_delta },
    { label: "Confidential", value: change.confidential_delta },
  ].filter(({ value }) => BigInt(value) !== 0n);

  return (
    <Stack spacing={0.25}>
      {values.map(({ label, value }) => (
        <Typography key={label} variant="body2" color={BigInt(value) > 0n ? "success.main" : "error.main"}>
          {values.length > 1 && `${label}: `}
          {formatSignedDelta(value, currency)}
        </Typography>
      ))}
    </Stack>
  );
}

function NewBalance({ change }: { change: BalanceChange }) {
  const currency = { symbol: change.token_symbol, decimals: change.divisibility };
  const showConfidential = BigInt(change.confidential_after) !== 0n || BigInt(change.confidential_delta) !== 0n;

  return (
    <Stack spacing={0.25}>
      <Typography variant="body2">
        {showConfidential && "Revealed: "}
        {formatCurrency(change.revealed_after, currency)}
      </Typography>
      {showConfidential && (
        <Typography variant="body2">Confidential: {formatCurrency(change.confidential_after, currency)}</Typography>
      )}
    </Stack>
  );
}

function Source({ change }: { change: BalanceChange }) {
  const source = change.source as { type?: string; transaction_id?: string };
  switch (source.type) {
    case "transaction": {
      const transactionId = source.transaction_id || change.transaction_id;
      if (!transactionId || !validateHash(transactionId)) {
        return <Typography variant="body2">Transaction (unavailable)</Typography>;
      }
      return (
        <MuiLink
          component={RouterLink}
          to={`/transactions/${transactionId}`}
          title={transactionId}
          color="secondary.light"
          underline="hover"
        >
          Transaction {shortenString(transactionId, 6, 6)}
        </MuiLink>
      );
    }
    case "scan":
      return <Typography variant="body2">Account scan</Typography>;
    case "recovery":
      return <Typography variant="body2">Account recovery</Typography>;
    default:
      return <Typography variant="body2">Unknown</Typography>;
  }
}

export function BalanceChangeHistory({ accountAddress, resourceAddress }: BalanceChangeHistoryProps) {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const { data, isLoading, isError, error, isRefetching } = useAccountsGetBalanceChanges(
    accountAddress,
    page * rowsPerPage,
    rowsPerPage,
    resourceAddress,
  );

  return (
    <FetchStatusCheck
      isLoading={isLoading && !isRefetching}
      isError={isError}
      errorMessage={error?.message || "Error fetching balance changes"}
    >
      <TableContainer>
        <Table
          aria-label={resourceAddress ? "Resource balance changes" : "Account balance changes"}
          sx={{ minWidth: 760 }}
        >
          <TableHead>
            <TableRow>
              <TableCell>Timestamp</TableCell>
              <TableCell>Resource</TableCell>
              <TableCell>Change</TableCell>
              <TableCell>New Balance</TableCell>
              <TableCell>Source</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {data?.changes.map((change) => (
              <TableRow key={change.id}>
                <DataTableCell>{formatTimestamp(change.created_at) || "Unknown"}</DataTableCell>
                <DataTableCell>
                  <CopyAddress
                    address={change.resource_address}
                    display={change.token_symbol || change.resource_address}
                  />
                </DataTableCell>
                <DataTableCell>
                  <ChangeValues change={change} />
                </DataTableCell>
                <DataTableCell>
                  <NewBalance change={change} />
                </DataTableCell>
                <DataTableCell>
                  <Source change={change} />
                </DataTableCell>
              </TableRow>
            ))}
            {data?.changes.length === 0 && (
              <TableRow>
                <TableCell colSpan={5} align="center">
                  No balance changes recorded yet.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
        <TablePagination
          rowsPerPageOptions={[5, 10, 25]}
          component="div"
          count={Number(data?.total ?? 0)}
          rowsPerPage={rowsPerPage}
          page={page}
          onPageChange={(_event, nextPage) => setPage(nextPage)}
          onRowsPerPageChange={(event) => {
            setRowsPerPage(Number.parseInt(event.target.value, 10));
            setPage(0);
          }}
        />
      </TableContainer>
    </FetchStatusCheck>
  );
}

interface BalanceChangeHistoryDialogProps extends BalanceChangeHistoryProps {
  open: boolean;
  onClose: () => void;
  resourceLabel?: string | null;
}

export function BalanceChangeHistoryDialog({
  open,
  onClose,
  accountAddress,
  resourceAddress,
  resourceLabel,
}: BalanceChangeHistoryDialogProps) {
  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="lg">
      <DialogTitle>Balance changes{resourceLabel ? ` · ${resourceLabel}` : ""}</DialogTitle>
      <DialogContent dividers>
        <BalanceChangeHistory accountAddress={accountAddress} resourceAddress={resourceAddress} />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Close</Button>
      </DialogActions>
    </Dialog>
  );
}
