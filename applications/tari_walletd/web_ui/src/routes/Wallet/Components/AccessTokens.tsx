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

import {
  useAuthRevokeToken,
  useCreateApiKey,
  useGetAllTokens,
  useListApiKeys,
  useRevokeApiKey,
} from "@api/hooks/useTokens";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { AccordionIconButton, CodeBlock, DataTableCell } from "@components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import {
  Fade,
  Checkbox,
  FormControlLabel,
  List,
  ListItem,
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
import Button from "@mui/material/Button";
import Collapse from "@mui/material/Collapse";
import Dialog from "@mui/material/Dialog";
import DialogActions from "@mui/material/DialogActions";
import DialogContent from "@mui/material/DialogContent";
import DialogContentText from "@mui/material/DialogContentText";
import DialogTitle from "@mui/material/DialogTitle";
import IconButton from "@mui/material/IconButton";
import type {
  AuthSessionInfo,
  JrpcPermission,
  JrpcPermissions,
  RefreshTokenHash,
} from "@tari-project/ootle-ts-bindings";
import { jrpcPermissionToString } from "@tari-project/ootle-ts-bindings";
import { useState } from "react";
import { IoCloseCircleOutline } from "react-icons/io5";

const API_KEY_PERMISSIONS: JrpcPermission[] = [
  "AccountInfo",
  "AccountList",
  "SubstatesRead",
  "TemplatesRead",
  "KeyList",
  "TransactionGet",
  "TransactionSend",
  "GetNft",
  "Admin",
];

function AlertDialog({ fn }: any) {
  const [open, setOpen] = useState(false);

  const handleClickOpen = () => {
    setOpen(true);
  };

  const handleClose = () => {
    setOpen(false);
  };

  const handleRevokeClose = () => {
    fn();
    setOpen(false);
  };

  return (
    <div>
      <IconButton onClick={handleClickOpen} color="primary">
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
          <Button variant="outlined" onClick={handleClose}>
            No, Cancel
          </Button>
          <Button variant="contained" onClick={handleRevokeClose} autoFocus>
            Yes, Revoke
          </Button>
        </DialogActions>
      </Dialog>
    </div>
  );
}

export default function AccessTokens() {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const { data, isLoading, error, isError } = useGetAllTokens();
  const { mutate } = useAuthRevokeToken();

  const handleRevoke = async (id: RefreshTokenHash) => {
    mutate(id);
  };

  const emptyRows = page > 0 ? Math.max(0, (1 + page) * rowsPerPage - (data?.sessions.length || 0)) : 0;

  const handleChangePage = (_event: unknown, newPage: number) => {
    setPage(newPage);
  };

  const handleChangeRowsPerPage = (event: React.ChangeEvent<HTMLInputElement>) => {
    setRowsPerPage(parseInt(event.target.value, 10));
    setPage(0);
  };

  function RowData({
    id,
    permissions,
    formattedDate,
  }: {
    id: RefreshTokenHash;
    permissions: JrpcPermissions;
    formattedDate: string;
  }) {
    const [open, setOpen] = useState(false);

    return (
      <>
        <TableRow key={id}>
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
            <AlertDialog fn={() => handleRevoke(id)} />
          </DataTableCell>
        </TableRow>
        <TableRow>
          <DataTableCell
            style={{
              paddingBottom: 0,
              paddingTop: 0,
            }}
            colSpan={5}
          >
            <Collapse in={open} timeout="auto" unmountOnExit>
              <CodeBlock style={{ marginBottom: "10px" }}>
                Permissions:
                <List>
                  {permissions.map((item: JrpcPermission) => {
                    let permission = jrpcPermissionToString(item);
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

  return (
    <FetchStatusCheck isLoading={isLoading} isError={isError} errorMessage={error?.message || "Error fetching data"}>
      <Fade in={!isLoading && !isError}>
        <Stack spacing={3}>
          <ApiKeysSection />
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
              {data?.sessions
                ?.slice(page * rowsPerPage, page * rowsPerPage + rowsPerPage)
                .map(({ id, permissions, exp }: AuthSessionInfo) => {
                  const date = new Date(Number(exp) * 1000);
                  const formattedDate = `${date.toISOString().slice(0, 10)} ${date.toISOString().slice(11, 16)}`;
                  return <RowData key={id} id={id} permissions={permissions} formattedDate={formattedDate} />;
                })}

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
                count={data.sessions.length}
                rowsPerPage={rowsPerPage}
                page={page}
                onPageChange={handleChangePage}
                onRowsPerPageChange={handleChangeRowsPerPage}
              />
            )}
          </TableContainer>
        </Stack>
      </Fade>
    </FetchStatusCheck>
  );
}

function ApiKeysSection() {
  const { data } = useListApiKeys();
  const createApiKey = useCreateApiKey();
  const revokeApiKey = useRevokeApiKey();
  const [name, setName] = useState("");
  const [selectedPermissions, setSelectedPermissions] = useState<JrpcPermission[]>(["AccountInfo"]);
  const [createdApiKey, setCreatedApiKey] = useState<string | null>(null);

  const togglePermission = (permission: JrpcPermission) => {
    setSelectedPermissions((current) =>
      current.includes(permission) ? current.filter((item) => item !== permission) : [...current, permission],
    );
  };

  const create = () => {
    createApiKey.mutate(
      {
        name,
        permissions: selectedPermissions,
        allow_admin: selectedPermissions.includes("Admin"),
      },
      {
        onSuccess: (response) => {
          setCreatedApiKey(response.api_key);
          setName("");
          setSelectedPermissions(["AccountInfo"]);
        },
      },
    );
  };

  return (
    <Stack spacing={2} sx={{ mb: 4 }}>
      <Typography variant="h6">API Keys</Typography>
      <Stack direction={{ xs: "column", md: "row" }} spacing={2} alignItems={{ xs: "stretch", md: "flex-start" }}>
        <TextField label="Name" value={name} onChange={(event) => setName(event.target.value)} size="small" />
        <Stack direction="row" flexWrap="wrap" gap={1}>
          {API_KEY_PERMISSIONS.map((permission) => (
            <FormControlLabel
              key={jrpcPermissionToString(permission)}
              control={
                <Checkbox
                  checked={selectedPermissions.includes(permission)}
                  onChange={() => togglePermission(permission)}
                  size="small"
                />
              }
              label={jrpcPermissionToString(permission)}
            />
          ))}
        </Stack>
        <Button
          variant="contained"
          onClick={create}
          disabled={!name.trim() || selectedPermissions.length === 0 || createApiKey.isPending}
        >
          Create
        </Button>
      </Stack>
      {createdApiKey && (
        <CodeBlock>
          <Stack spacing={1}>
            <Typography variant="body2">New API key</Typography>
            <Typography variant="body2">{createdApiKey}</Typography>
            <Button variant="outlined" onClick={() => setCreatedApiKey(null)}>
              Dismiss
            </Button>
          </Stack>
        </CodeBlock>
      )}
      <TableContainer>
        <Table>
          <TableHead>
            <TableRow>
              <TableCell>Name</TableCell>
              <TableCell>ID</TableCell>
              <TableCell>Created</TableCell>
              <TableCell>Last Used</TableCell>
              <TableCell>Permissions</TableCell>
              <TableCell width="100" align="center">
                Revoke
              </TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {data?.api_keys.map((apiKey) => (
              <TableRow key={apiKey.id}>
                <DataTableCell>{apiKey.name}</DataTableCell>
                <DataTableCell>{apiKey.id}</DataTableCell>
                <DataTableCell>{formatTimestamp(apiKey.created_at)}</DataTableCell>
                <DataTableCell>{apiKey.last_used_at ? formatTimestamp(apiKey.last_used_at) : ""}</DataTableCell>
                <DataTableCell>
                  {apiKey.permissions.map((permission: JrpcPermission) => jrpcPermissionToString(permission)).join(", ")}
                </DataTableCell>
                <DataTableCell sx={{ textAlign: "center" }}>
                  <IconButton onClick={() => revokeApiKey.mutate(apiKey.id)} color="primary">
                    <IoCloseCircleOutline />
                  </IconButton>
                </DataTableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableContainer>
    </Stack>
  );
}

function formatTimestamp(timestamp: number) {
  const date = new Date(Number(timestamp) * 1000);
  return `${date.toISOString().slice(0, 10)} ${date.toISOString().slice(11, 16)}`;
}
