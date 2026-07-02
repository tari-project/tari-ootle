//  Copyright 2026. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import FetchStatusCheck from "@components/FetchStatusCheck";
import { DataTableCell } from "@components/StyledComponents";
import { Table, TableBody, TableCell, TableContainer, TableHead, TableRow } from "@mui/material";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import DialogActions from "@mui/material/DialogActions";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import { useGetBalanceChanges } from "@api/hooks/useAccounts";
import { useTimeAgo } from "@hooks/useTimeAgo";
import { BalanceChangeEntry, BalanceChangeSource, ResourceAddress } from "@tari-project/ootle-ts-bindings";
import { Link } from "react-router-dom";

interface BalanceChangeDialogProps {
  open: boolean;
  onClose: () => void;
  account: string;
  resourceAddress: ResourceAddress;
  resourceLabel?: string;
}

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
  return null;
}

function TimestampCell({ created_at }: { created_at: string }) {
  const display = useTimeAgo(created_at);
  return <DataTableCell>{display}</DataTableCell>;
}

function BalanceChangeDialog({ open, onClose, account, resourceAddress, resourceLabel }: BalanceChangeDialogProps) {
  const { data, isLoading, isError, error } = useGetBalanceChanges(account, resourceAddress, undefined, 0, 50);

  const changes = data?.changes || [];

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle>Balance Changes{resourceLabel ? ` — ${resourceLabel}` : ""}</DialogTitle>
      <DialogContent>
        <FetchStatusCheck
          isError={isError}
          errorMessage={(error as { message?: string })?.message || "Error fetching balance changes"}
          isLoading={isLoading}
        >
          {changes.length === 0 ? (
            <DataTableCell>No balance changes found</DataTableCell>
          ) : (
            <TableContainer>
              <Table size="small">
                <TableHead>
                  <TableRow>
                    <TableCell>Time</TableCell>
                    <TableCell>Revealed Delta</TableCell>
                    <TableCell>Conf. Delta</TableCell>
                    <TableCell>Source</TableCell>
                    <TableCell>Transaction</TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {changes.map((entry: BalanceChangeEntry, i: number) => (
                    <TableRow key={`${entry.vault_address}_${entry.created_at}_${i}`}>
                      <TimestampCell created_at={entry.created_at} />
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
                  ))}
                </TableBody>
              </Table>
            </TableContainer>
          )}
        </FetchStatusCheck>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Close</Button>
      </DialogActions>
    </Dialog>
  );
}

export default BalanceChangeDialog;
