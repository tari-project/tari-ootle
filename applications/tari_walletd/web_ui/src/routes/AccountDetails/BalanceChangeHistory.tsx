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
  FormControl,
  InputLabel,
  MenuItem,
  Link as MuiLink,
  Select,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
  TextField,
  Typography,
} from "@mui/material";
import useAccountStore from "@store/accountStore";
import type {
  BalanceChange,
  BalanceChangeSourceType,
  ComponentAddress,
  ResourceAddress,
  TransactionId,
} from "@tari-project/ootle-ts-bindings";
import { shortenString } from "@tari-project/ootle-ts-bindings";
import { Currency } from "@utils/currency";
import { formatCurrency, formatTimestamp, validateHash } from "@utils/helpers";
import { type FormEvent, useId, useState } from "react";
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
  return `${sign}${formatCurrency(magnitude, currency).trimEnd()}`;
}

function ChangeValues({ change, showBalance }: { change: BalanceChange; showBalance: boolean }) {
  if (!showBalance) {
    return <Typography variant="body2">********</Typography>;
  }

  const currency = { symbol: change.token_symbol, decimals: change.divisibility };
  const values = [
    { label: "Revealed", value: change.revealed_delta },
    { label: "Confidential", value: change.confidential_delta },
  ].filter(({ value }) => BigInt(value) !== 0n);

  return (
    <Stack spacing={0.25}>
      {values.map(({ label, value }) => (
        <Typography key={label} variant="body2" color={BigInt(value) > 0n ? "success.main" : "error.main"}>
          {`${label}: ${formatSignedDelta(value, currency)}`}
        </Typography>
      ))}
    </Stack>
  );
}

function NewBalance({ change, showBalance }: { change: BalanceChange; showBalance: boolean }) {
  if (!showBalance) {
    return <Typography variant="body2">********</Typography>;
  }

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
  const { source } = change;
  switch (source.type) {
    case "Transaction": {
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
    case "Scan":
      return <Typography variant="body2">Account scan</Typography>;
    case "Recovery":
      return <Typography variant="body2">Account recovery</Typography>;
    default:
      return <Typography variant="body2">Unknown source</Typography>;
  }
}

export function BalanceChangeHistory({ accountAddress, resourceAddress }: BalanceChangeHistoryProps) {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [sourceDraft, setSourceDraft] = useState<BalanceChangeSourceType | "">("");
  const [transactionDraft, setTransactionDraft] = useState("");
  const [sourceType, setSourceType] = useState<BalanceChangeSourceType>();
  const [transactionId, setTransactionId] = useState<TransactionId>();
  const sourceFilterLabelId = useId();
  const showBalance = useAccountStore((state) => state.showBalance);
  const trimmedTransaction = transactionDraft.trim();
  const transactionIsInvalid = trimmedTransaction !== "" && !validateHash(trimmedTransaction);
  const { data, isLoading, isError, error, isRefetching } = useAccountsGetBalanceChanges(
    accountAddress,
    page * rowsPerPage,
    rowsPerPage,
    resourceAddress,
    sourceType,
    transactionId,
  );

  const applyFilters = (event: FormEvent) => {
    event.preventDefault();
    if (transactionIsInvalid) return;
    setSourceType(sourceDraft || undefined);
    setTransactionId(trimmedTransaction ? (trimmedTransaction as TransactionId) : undefined);
    setPage(0);
  };

  const clearFilters = () => {
    setSourceDraft("");
    setTransactionDraft("");
    setSourceType(undefined);
    setTransactionId(undefined);
    setPage(0);
  };

  return (
    <FetchStatusCheck
      isLoading={isLoading && !isRefetching}
      isError={isError}
      errorMessage={error?.message || "Error fetching balance changes"}
    >
      <Stack
        component="form"
        onSubmit={applyFilters}
        direction={{ xs: "column", md: "row" }}
        spacing={1.5}
        alignItems={{ md: "flex-start" }}
        mb={2}
      >
        <FormControl size="small" sx={{ minWidth: 180 }}>
          <InputLabel id={sourceFilterLabelId}>Source</InputLabel>
          <Select
            labelId={sourceFilterLabelId}
            label="Source"
            value={sourceDraft}
            onChange={(event) => setSourceDraft(event.target.value as BalanceChangeSourceType | "")}
          >
            <MenuItem value="">All sources</MenuItem>
            <MenuItem value="Transaction">Transaction</MenuItem>
            <MenuItem value="Scan">Account scan</MenuItem>
            <MenuItem value="Recovery">Account recovery</MenuItem>
          </Select>
        </FormControl>
        <TextField
          size="small"
          label="Transaction ID"
          value={transactionDraft}
          onChange={(event) => setTransactionDraft(event.target.value)}
          error={transactionIsInvalid}
          helperText={transactionIsInvalid ? "Enter a 64-character hexadecimal transaction ID" : " "}
          sx={{ flexGrow: 1, minWidth: 280 }}
        />
        <Button type="submit" variant="outlined" disabled={transactionIsInvalid}>
          Apply
        </Button>
        <Button type="button" variant="text" onClick={clearFilters}>
          Clear
        </Button>
      </Stack>
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
                  <CopyAddress address={change.resource_address} display={change.token_symbol || undefined} />
                </DataTableCell>
                <DataTableCell>
                  <ChangeValues change={change} showBalance={showBalance} />
                </DataTableCell>
                <DataTableCell>
                  <NewBalance change={change} showBalance={showBalance} />
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

interface BalanceChangeHistoryActionProps {
  accountAddress: ComponentAddress;
  resourceAddress: ResourceAddress;
  resourceLabel?: string | null;
}

export function BalanceChangeHistoryAction({
  accountAddress,
  resourceAddress,
  resourceLabel,
}: BalanceChangeHistoryActionProps) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <Button size="small" variant="text" onClick={() => setOpen(true)}>
        View changes
      </Button>
      {open && (
        <BalanceChangeHistoryDialog
          open
          onClose={() => setOpen(false)}
          accountAddress={accountAddress}
          resourceAddress={resourceAddress}
          resourceLabel={resourceLabel}
        />
      )}
    </>
  );
}
