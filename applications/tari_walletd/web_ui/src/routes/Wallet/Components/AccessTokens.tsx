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

import { useAuthRevokeToken, useGetAllTokens } from "@api/hooks/useTokens";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { AccordionIconButton, CodeBlock, DataTableCell } from "@components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import {
  Fade,
  List,
  ListItem,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
} from "@mui/material";
import Button from "@mui/material/Button";
import Collapse from "@mui/material/Collapse";
import Dialog from "@mui/material/Dialog";
import DialogActions from "@mui/material/DialogActions";
import DialogContent from "@mui/material/DialogContent";
import DialogContentText from "@mui/material/DialogContentText";
import DialogTitle from "@mui/material/DialogTitle";
import IconButton from "@mui/material/IconButton";
import type { AuthSessionInfo, Permission, Permissions, RefreshTokenHash } from "@tari-project/ootle-ts-bindings";
import { permissionToString } from "@tari-project/ootle-ts-bindings";
import { useEffect, useState } from "react";
import { IoCloseCircleOutline } from "react-icons/io5";
import { useErrorNotification } from "../../../contexts/ErrorNotificationContext";

function formatSessionExpiry(exp: bigint | number): string {
  const date = new Date(Number(exp) * 1000);
  if (Number.isNaN(date.getTime())) {
    return "Unknown";
  }
  return `${date.toISOString().slice(0, 10)} ${date.toISOString().slice(11, 16)}`;
}

function AlertDialog({ onConfirm, isRevoking }: { onConfirm: () => Promise<boolean>; isRevoking: boolean }) {
  const [open, setOpen] = useState(false);

  const handleClickOpen = () => {
    setOpen(true);
  };

  const handleClose = () => {
    if (!isRevoking) {
      setOpen(false);
    }
  };

  const handleRevokeClose = async () => {
    if (isRevoking) {
      return;
    }
    const revoked = await onConfirm();
    if (revoked) {
      setOpen(false);
    }
  };

  return (
    <div>
      <IconButton aria-label="Revoke token" onClick={handleClickOpen} color="primary" disabled={isRevoking}>
        <IoCloseCircleOutline />
      </IconButton>
      <Dialog
        open={open}
        onClose={handleClose}
        aria-labelledby="alert-dialog-title"
        aria-describedby="alert-dialog-description"
      >
        <DialogTitle id="alert-dialog-title">Revoke Token</DialogTitle>
        <DialogContent>
          <DialogContentText id="alert-dialog-description">Would you like to revoke this token?</DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button variant="outlined" onClick={handleClose} disabled={isRevoking}>
            No, Cancel
          </Button>
          <Button variant="contained" onClick={handleRevokeClose} disabled={isRevoking} autoFocus>
            {isRevoking ? "Revoking..." : "Yes, Revoke"}
          </Button>
        </DialogActions>
      </Dialog>
    </div>
  );
}

function RowData({
  id,
  permissions,
  formattedDate,
  onRevoke,
  isRevoking,
}: {
  id: RefreshTokenHash;
  permissions: Permissions;
  formattedDate: string;
  onRevoke: (id: RefreshTokenHash) => Promise<boolean>;
  isRevoking: boolean;
}) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <TableRow>
        <DataTableCell
          style={{
            borderBottom: "none",
          }}
        >
          {id}
        </DataTableCell>
        <DataTableCell
          style={{
            borderBottom: "none",
          }}
        >
          {formattedDate}
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: "none", textAlign: "center" }}>
          <AccordionIconButton
            aria-label="expand row"
            size="small"
            onClick={() => {
              setOpen(!open);
            }}
          >
            {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
        <DataTableCell sx={{ borderBottom: "none", textAlign: "center" }}>
          <AlertDialog onConfirm={() => onRevoke(id)} isRevoking={isRevoking} />
        </DataTableCell>
      </TableRow>
      <TableRow>
        <DataTableCell
          style={{
            paddingBottom: 0,
            paddingTop: 0,
          }}
          colSpan={4}
        >
          <Collapse in={open} timeout="auto" unmountOnExit>
            <CodeBlock style={{ marginBottom: "10px" }}>
              Permissions:
              <List>
                {permissions.map((item: Permission) => {
                  let permission = permissionToString(item);
                  return <ListItem key={permission}>{permission}</ListItem>;
                })}
              </List>
            </CodeBlock>
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

export default function AccessTokens() {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const { data, isLoading, error, isError } = useGetAllTokens();
  const { mutateAsync: revokeToken, isPending: isRevoking } = useAuthRevokeToken();
  const { showError, showSuccess } = useErrorNotification();

  const sessions = data?.sessions ?? [];
  const maxPage = Math.max(0, Math.ceil(sessions.length / rowsPerPage) - 1);
  const effectivePage = Math.min(page, maxPage);

  useEffect(() => {
    if (page > maxPage) {
      setPage(maxPage);
    }
  }, [page, maxPage]);

  const handleRevoke = async (id: RefreshTokenHash): Promise<boolean> => {
    try {
      await revokeToken(id);
      showSuccess("Access token revoked");
      return true;
    } catch (err: unknown) {
      showError(err instanceof Error && err.message ? err.message : "Failed to revoke access token");
      return false;
    }
  };

  const emptyRows = effectivePage > 0 ? Math.max(0, (1 + effectivePage) * rowsPerPage - sessions.length) : 0;

  const handleChangePage = (_event: unknown, newPage: number) => {
    setPage(newPage);
  };

  const handleChangeRowsPerPage = (event: React.ChangeEvent<HTMLInputElement>) => {
    setRowsPerPage(parseInt(event.target.value, 10));
    setPage(0);
  };

  return (
    <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message || "Error fetching data"}>
      <Fade in={!isLoading && !isError}>
        <TableContainer>
          <Table>
            <TableHead>
              <TableRow>
                <TableCell>ID</TableCell>
                <TableCell>Expiry Date</TableCell>
                <TableCell align="center">Permissions</TableCell>
                <TableCell width="100" align="center">
                  Revoke
                </TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {sessions
                .slice(effectivePage * rowsPerPage, effectivePage * rowsPerPage + rowsPerPage)
                .map(({ id, permissions, exp }: AuthSessionInfo) => (
                  <RowData
                    key={id}
                    id={id}
                    permissions={permissions}
                    formattedDate={formatSessionExpiry(exp)}
                    onRevoke={handleRevoke}
                    isRevoking={isRevoking}
                  />
                ))}

              {emptyRows > 0 && (
                <TableRow style={{ height: 57 * emptyRows }}>
                  <TableCell colSpan={4} />
                </TableRow>
              )}
            </TableBody>
          </Table>
          {data?.sessions && (
            <TablePagination
              rowsPerPageOptions={[10, 25, 50]}
              component="div"
              count={sessions.length}
              rowsPerPage={rowsPerPage}
              page={effectivePage}
              onPageChange={handleChangePage}
              onRowsPerPageChange={handleChangeRowsPerPage}
            />
          )}
        </TableContainer>
      </Fade>
    </FetchStatusCheck>
  );
}
